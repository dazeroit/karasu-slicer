//! Deterministic data parallelism for large analysis arrays (sorting and per-item maps).
//!
//! Results never depend on the thread count: sorts produce exactly the sequential order whenever
//! elements with equal keys are equal (callers sort by unique keys or by whole values), and maps
//! write each item from a pure function of its own input. Work runs on scoped threads; only
//! [`fill_balanced_cancellable`] polls cancellation, the others' callers check it before and after
//! each call.

use crate::cancellation::{Cancellation, Cancelled, Checkpoint};
use std::{
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

/// Smallest slice worth one thread: below it the calling thread does the work alone.
const MIN_PER_THREAD: usize = 1 << 15;

/// Threads for `len` items: every available core, but at least `MIN_PER_THREAD` items each.
fn threads_for(len: usize) -> usize {
    thread::available_parallelism()
        .map_or(1, usize::from)
        .min(len / MIN_PER_THREAD)
        .max(1)
}

/// Sort `items` by `key`, using every available core for large slices.
///
/// The result equals `items.sort_unstable_by_key(key)` whenever elements with equal keys are
/// equal, which every caller guarantees (unique keys, or keys covering the whole element). When
/// the merge buffer cannot be allocated the sort runs sequentially, with the same result.
pub(crate) fn sort_by_key<T, K, F>(items: &mut [T], key: F)
where
    T: Copy + Send + Sync,
    K: Ord,
    F: Fn(&T) -> K + Sync,
{
    let threads = threads_for(items.len());
    let mut buffer = Vec::new();
    if threads == 1 || buffer.try_reserve_exact(items.len()).is_err() {
        items.sort_unstable_by_key(key);
        return;
    }
    let key = &key;
    let width = items.len().div_ceil(threads);
    thread::scope(|scope| {
        for part in items.chunks_mut(width) {
            scope.spawn(move || part.sort_unstable_by_key(key));
        }
    });
    buffer.extend_from_slice(items);
    // Bottom-up merges of sorted runs, alternating between the two arrays.
    let (mut width, mut in_items) = (width, true);
    while width < items.len() {
        if in_items {
            merge_round(items, &mut buffer, width, threads, key);
        } else {
            merge_round(&buffer, items, width, threads, key);
        }
        in_items = !in_items;
        width *= 2;
    }
    if !in_items {
        items.copy_from_slice(&buffer);
    }
}

/// Merge each pair of adjacent sorted runs of `width` from `from` into `to`, splitting large
/// merges into independent output segments so every round uses all threads.
fn merge_round<T, K, F>(from: &[T], to: &mut [T], width: usize, threads: usize, key: &F)
where
    T: Copy + Send + Sync,
    K: Ord,
    F: Fn(&T) -> K + Sync,
{
    let pairs = from.len().div_ceil(2 * width);
    let segments = threads.div_ceil(pairs).max(1);
    thread::scope(|scope| {
        for (source, mut target) in from.chunks(2 * width).zip(to.chunks_mut(2 * width)) {
            let (left, right) = source.split_at(width.min(source.len()));
            let total = source.len();
            let mut done = (0, 0);
            for segment in 1..=segments {
                let end = total * segment / segments;
                let i = co_rank(end, left, right, key);
                let next = (i, end - i);
                let (out, rest) = std::mem::take(&mut target).split_at_mut(end - (done.0 + done.1));
                target = rest;
                let (a, b) = (&left[done.0..next.0], &right[done.1..next.1]);
                scope.spawn(move || merge(a, b, out, key));
                done = next;
            }
        }
    });
}

/// How many of the first `k` merged elements come from `left`, with ties taken from `left`.
fn co_rank<T, K: Ord>(k: usize, left: &[T], right: &[T], key: &impl Fn(&T) -> K) -> usize {
    let (mut low, mut high) = (k.saturating_sub(right.len()), k.min(left.len()));
    while low < high {
        let i = usize::midpoint(low, high);
        let j = k - i;
        // Too few from `left` when its next element must precede the last one taken from `right`.
        if j > 0 && key(&left[i]) <= key(&right[j - 1]) {
            low = i + 1;
        } else {
            high = i;
        }
    }
    low
}

/// Stable two-way merge of sorted `left` and `right` into `out` (exactly their combined length).
fn merge<T: Copy, K: Ord>(left: &[T], right: &[T], out: &mut [T], key: &impl Fn(&T) -> K) {
    let (mut i, mut j) = (0, 0);
    for slot in out {
        let take_left = j == right.len() || (i < left.len() && key(&left[i]) <= key(&right[j]));
        if take_left {
            *slot = left[i];
            i += 1;
        } else {
            *slot = right[j];
            j += 1;
        }
    }
}

/// Fill `out[i] = f(i)` using every available core for large slices.
pub(crate) fn fill_with<T, F>(out: &mut [T], f: F)
where
    T: Send,
    F: Fn(usize) -> T + Sync,
{
    fill_in_threads(out, threads_for(out.len()), f);
}

/// Fill `out[i] = f(i)` with one thread per available core, however few the items: for items
/// that each cost much (a whole layer).
pub(crate) fn fill_costly_with<T, F>(out: &mut [T], f: F)
where
    T: Send,
    F: Fn(usize) -> T + Sync,
{
    let cores = thread::available_parallelism().map_or(1, usize::from);
    fill_in_threads(out, cores.min(out.len()).max(1), f);
}

/// How often the caller's thread polls cancellation while [`fill_balanced_cancellable`]'s
/// workers run.
const POLL: Duration = Duration::from_millis(20);

/// Fill `out[i] = f(i, halted)` with one thread per available core, each taking the next index as
/// soon as it is done with one: for items of very unequal cost. The result does not depend on the
/// number of threads or on which thread computed what.
///
/// While the workers run, the caller's thread polls `cancel` at `checkpoint` every [`POLL`], and
/// when it is cancelled raises the flag `f` is given, so an item that takes long can stop early;
/// no new item starts after that. On cancellation `out` is left partly filled and must be
/// discarded.
///
/// # Errors
/// [`Cancelled`], before the work or while it runs.
pub(crate) fn fill_balanced_cancellable<T, F>(
    out: &mut [T],
    cancel: &mut Cancellation<'_>,
    checkpoint: Checkpoint,
    f: F,
) -> Result<(), Cancelled>
where
    T: Send,
    F: Fn(usize, &AtomicBool) -> T + Sync,
{
    cancel.check(checkpoint)?;
    let cores = thread::available_parallelism().map_or(1, usize::from);
    let threads = cores.min(out.len()).max(1);
    let (f, len) = (&f, out.len());
    let (next, halted) = (AtomicUsize::new(0), AtomicBool::new(false));
    let (next, halted) = (&next, &halted);
    let mut stopped = None;
    let done: Vec<Vec<(usize, T)>> = thread::scope(|scope| {
        let (sender, receiver) = mpsc::channel::<()>();
        let workers: Vec<_> = (0..threads)
            .map(|_| {
                // Dropped when the worker ends, however it ends: the caller then stops waiting.
                let sender = sender.clone();
                scope.spawn(move || {
                    let _sender = sender;
                    let mut local = Vec::new();
                    while !halted.load(Ordering::Relaxed) {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        if i >= len {
                            break;
                        }
                        local.push((i, f(i, halted)));
                    }
                    local
                })
            })
            .collect();
        drop(sender);
        while let Err(mpsc::RecvTimeoutError::Timeout) = receiver.recv_timeout(POLL) {
            if stopped.is_none() {
                if let Err(cancelled) = cancel.check(checkpoint) {
                    stopped = Some(cancelled);
                    halted.store(true, Ordering::Relaxed);
                }
            }
        }
        workers
            .into_iter()
            .map(|w| {
                w.join()
                    .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
            })
            .collect()
    });
    if let Some(cancelled) = stopped {
        return Err(cancelled);
    }
    for (i, value) in done.into_iter().flatten() {
        out[i] = value;
    }
    Ok(())
}

/// Fill `out[i] = f(i)` on `threads` threads, each a contiguous part.
fn fill_in_threads<T, F>(out: &mut [T], threads: usize, f: F)
where
    T: Send,
    F: Fn(usize) -> T + Sync,
{
    if threads == 1 {
        for (i, slot) in out.iter_mut().enumerate() {
            *slot = f(i);
        }
        return;
    }
    let f = &f;
    let width = out.len().div_ceil(threads);
    thread::scope(|scope| {
        for (chunk, part) in out.chunks_mut(width).enumerate() {
            scope.spawn(move || {
                for (i, slot) in part.iter_mut().enumerate() {
                    *slot = f(chunk * width + i);
                }
            });
        }
    });
}

/// The smallest index in `0..len` where `found` holds, searching on every available core.
pub(crate) fn first_index<F>(len: usize, found: F) -> Option<usize>
where
    F: Fn(usize) -> bool + Sync,
{
    let threads = threads_for(len);
    if threads == 1 {
        return (0..len).find(|&i| found(i));
    }
    let found = &found;
    let width = len.div_ceil(threads);
    thread::scope(|scope| {
        let handles: Vec<_> = (0..threads)
            .map(|chunk| {
                scope.spawn(move || {
                    (chunk * width..((chunk + 1) * width).min(len)).find(|&i| found(i))
                })
            })
            .collect();
        // Chunks are in index order, so the first chunk with a hit holds the smallest index.
        handles
            .into_iter()
            .map(|h| h.join().ok().flatten())
            .find(Option::is_some)
            .flatten()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cancellable_fill_fills_in_order_and_stops_running_items() {
        let mut out = vec![0_usize; 1_000];
        fill_balanced_cancellable(
            &mut out,
            &mut Cancellation::never(),
            Checkpoint::BooleanCells,
            |i, _| i * i,
        )
        .unwrap();
        assert!(out.iter().enumerate().all(|(i, &v)| v == i * i));
        // Items that would run until halted: the caller's second poll cancels them, and every
        // one that started saw the flag.
        let mut polls = 0;
        let mut predicate = |_| {
            polls += 1;
            polls > 1
        };
        let stopped = AtomicUsize::new(0);
        let mut out = vec![(); 64];
        let error = fill_balanced_cancellable(
            &mut out,
            &mut Cancellation::new(&mut predicate),
            Checkpoint::BooleanCells,
            |_, halted| {
                while !halted.load(Ordering::Relaxed) {
                    thread::yield_now();
                }
                stopped.fetch_add(1, Ordering::Relaxed);
            },
        )
        .unwrap_err();
        assert_eq!(error.checkpoint, Checkpoint::BooleanCells);
        let cores = thread::available_parallelism().map_or(1, usize::from);
        assert!(stopped.load(Ordering::Relaxed) <= cores.min(64));
    }

    fn random(n: usize, modulus: u64, seed: u64) -> Vec<(u64, u32)> {
        let mut state = seed | 1;
        (0..n)
            .map(|i| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                (state % modulus, u32::try_from(i).unwrap())
            })
            .collect()
    }

    #[test]
    fn parallel_sort_equals_the_sequential_sort() {
        // Sizes around the thread threshold, many and few distinct keys, sorted and reversed input.
        for (case, n) in [0, 1, 7, MIN_PER_THREAD - 1, MIN_PER_THREAD * 3 + 5, 400_003]
            .into_iter()
            .enumerate()
        {
            for modulus in [3, 1000, u64::MAX] {
                let base = random(n, modulus, (case as u64) ^ modulus);
                for variant in 0..3 {
                    let mut input = base.clone();
                    match variant {
                        1 => input.sort_unstable(),
                        2 => input.sort_unstable_by(|a, b| b.cmp(a)),
                        _ => {}
                    }
                    // Unique keys (value, index): the order is fully determined.
                    let mut expected = input.clone();
                    expected.sort_unstable_by_key(|&(v, i)| (v, i));
                    let mut actual = input.clone();
                    sort_by_key(&mut actual, |&(v, i)| (v, i));
                    assert_eq!(
                        actual, expected,
                        "n {n} modulus {modulus} variant {variant}"
                    );
                    // Keys covering the whole element: equal keys are equal elements.
                    let mut values: Vec<u64> = input.iter().map(|&(v, _)| v).collect();
                    let mut sorted = values.clone();
                    sorted.sort_unstable();
                    sort_by_key(&mut values, |&v| v);
                    assert_eq!(values, sorted);
                }
            }
        }
    }

    #[test]
    fn co_rank_takes_ties_from_the_left() {
        let (left, right) = ([1, 2, 2, 5], [2, 2, 3]);
        let key = |v: &i32| *v;
        let merged = [1, 2, 2, 2, 2, 3, 5];
        for k in 0..=7 {
            let i = co_rank(k, &left, &right, &key);
            let mut out = vec![0; k];
            merge(&left[..i], &right[..k - i], &mut out, &key);
            assert_eq!(out, merged[..k]);
        }
        assert_eq!(co_rank(3, &left, &right, &key), 3);
    }

    #[test]
    fn first_index_finds_the_smallest_match() {
        for n in [0, 10, MIN_PER_THREAD * 5 + 7] {
            for target in [None, Some(0), Some(n / 3), Some(n.saturating_sub(1))] {
                let target = target.filter(|&t| t < n);
                // Every index from the target on matches, so only the smallest is correct.
                let found = first_index(n, |i| target.is_some_and(|t| i >= t));
                assert_eq!(found, target, "n {n}");
            }
        }
    }

    #[test]
    fn fill_with_is_index_exact() {
        for n in [0, 5, MIN_PER_THREAD * 4 + 3] {
            let mut out = vec![0usize; n];
            fill_with(&mut out, |i| i * 3 + 1);
            assert!(out.iter().enumerate().all(|(i, &v)| v == i * 3 + 1));
        }
    }
}
