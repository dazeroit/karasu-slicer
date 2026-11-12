//! Exact parallel vertex welding (TD-058).
//!
//! **Rule:** corner positions with equal coordinate keys ([`Point3::key`]: exact bits, signed zero
//! normalized) become one vertex. Vertices are numbered in order of first occurrence, exactly as a
//! sequential hash-map builder numbers them, so the welded mesh does not depend on the thread
//! count.
//!
//! **Method:**
//! - Corners are partitioned into shards by a hash of their key, on every available core.
//! - Each shard visits its corners in increasing order, with its own hash map, and records each
//!   corner's first equal corner.
//! - One sequential pass then numbers the first corners and maps every corner to its vertex.
//!
//! **Hashing:** a fast multiply-mix hash keyed by a per-process random seed, so crafted files cannot
//! predict collisions.
//!
//! **Memory:** about 9 bytes per corner of scratch beyond the hash maps.

use crate::geometry::Point3;
use std::collections::HashMap;
use std::hash::{BuildHasher, Hash, Hasher, RandomState};
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::thread;

/// Corners per thread below which welding stays on the calling thread.
const MIN_PER_THREAD: usize = 1 << 16;
/// Shards (a power of two), several per thread for load balance.
const SHARDS: usize = 64;

/// An exact coordinate key with a three-word hash.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Key([u64; 3]);

impl Hash for Key {
    fn hash<H: Hasher>(&self, state: &mut H) {
        for word in self.0 {
            state.write_u64(word);
        }
    }
}

/// Multiply-mix hasher seeded per process; only `write_u64` is used by [`Key`].
#[derive(Clone, Copy)]
struct MixHasher(u64);

impl Hasher for MixHasher {
    fn write(&mut self, bytes: &[u8]) {
        for chunk in bytes.chunks(8) {
            let mut word = [0u8; 8];
            word[..chunk.len()].copy_from_slice(chunk);
            self.write_u64(u64::from_le_bytes(word));
        }
    }

    fn write_u64(&mut self, word: u64) {
        self.0 = (self.0 ^ word)
            .wrapping_mul(0x9e37_79b9_7f4a_7c15)
            .rotate_left(31);
    }

    fn finish(&self) -> u64 {
        // SplitMix64 finalizer: every input bit affects every output bit.
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
}

/// Builds [`MixHasher`]s from one seed.
#[derive(Clone, Copy)]
struct Seeded(u64);

impl Seeded {
    fn new() -> Self {
        Self(RandomState::new().hash_one(0x6b61_7261_7375_u64))
    }

    /// An independent hash for shard selection, so shards do not concentrate a map's buckets.
    fn shard(self, key: Key) -> usize {
        let mut hasher = MixHasher(self.0 ^ 0x5851_f42d_4c95_7f2d);
        key.hash(&mut hasher);
        // The top bits of the finished hash select one of `SHARDS` (a power of two).
        usize::try_from(hasher.finish() >> (64 - SHARDS.trailing_zeros())).unwrap_or(0)
    }
}

impl BuildHasher for Seeded {
    type Hasher = MixHasher;
    fn build_hasher(&self) -> MixHasher {
        MixHasher(self.0)
    }
}

/// The welded vertices and, for every corner, its vertex index.
pub(crate) struct Welded {
    pub(crate) vertices: Vec<Point3>,
    pub(crate) corner_vertex: Vec<u32>,
}

/// Why welding failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WeldError {
    /// More distinct positions than the cap.
    TooManyVertices,
    /// A buffer could not be allocated, or a count does not fit `u32`.
    Allocation,
}

fn reserve<T>(items: &mut Vec<T>, count: usize) -> Result<(), WeldError> {
    items
        .try_reserve_exact(count)
        .map_err(|_| WeldError::Allocation)
}

/// Weld `count` corners, where corner `c` is at `point(c)` (finite coordinates).
pub(crate) fn weld(
    count: usize,
    point: &(dyn Fn(usize) -> Point3 + Sync),
    max_vertices: usize,
) -> Result<Welded, WeldError> {
    if u32::try_from(count).is_err() {
        return Err(WeldError::Allocation);
    }
    let seeded = Seeded::new();
    let threads = thread::available_parallelism()
        .map_or(1, usize::from)
        .min(count / MIN_PER_THREAD)
        .max(1);
    let first = if threads == 1 {
        first_sequential(count, point, seeded)?
    } else {
        first_parallel(count, point, seeded, threads)?
    };
    number(first, point, max_vertices)
}

/// For every corner, the index of the first corner with an equal key.
fn first_sequential(
    count: usize,
    point: &(dyn Fn(usize) -> Point3 + Sync),
    seeded: Seeded,
) -> Result<Vec<u32>, WeldError> {
    let mut first = Vec::new();
    reserve(&mut first, count)?;
    let mut map: HashMap<Key, u32, Seeded> = HashMap::with_hasher(seeded);
    for corner in 0..count {
        // `weld` checked that every corner index fits u32.
        let index = u32::try_from(corner).map_err(|_| WeldError::Allocation)?;
        map.try_reserve(1).map_err(|_| WeldError::Allocation)?;
        first.push(*map.entry(Key(point(corner).key())).or_insert(index));
    }
    Ok(first)
}

/// [`first_sequential`] on `threads` threads, with the same result.
fn first_parallel(
    count: usize,
    point: &(dyn Fn(usize) -> Point3 + Sync),
    seeded: Seeded,
    threads: usize,
) -> Result<Vec<u32>, WeldError> {
    // Pass 1: each chunk lists its corners per shard, in increasing order.
    let chunks = threads * 4;
    let width = count.div_ceil(chunks);
    let lists: Vec<Result<Vec<Vec<u32>>, WeldError>> = thread::scope(|scope| {
        let handles: Vec<_> = (0..chunks)
            .map(|chunk| {
                scope.spawn(move || {
                    let (start, end) = (chunk * width, ((chunk + 1) * width).min(count));
                    let mut lists: Vec<Vec<u32>> = (0..SHARDS).map(|_| Vec::new()).collect();
                    for corner in start..end {
                        let shard = seeded.shard(Key(point(corner).key()));
                        let index = u32::try_from(corner).map_err(|_| WeldError::Allocation)?;
                        lists[shard]
                            .try_reserve(1)
                            .map_err(|_| WeldError::Allocation)?;
                        lists[shard].push(index);
                    }
                    Ok(lists)
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().unwrap_or(Err(WeldError::Allocation)))
            .collect()
    });
    let lists: Vec<Vec<Vec<u32>>> = lists.into_iter().collect::<Result<_, _>>()?;
    // Pass 2: shards in parallel; within a shard, chunks and corners in increasing order.
    let mut first = Vec::new();
    reserve(&mut first, count)?;
    first.extend((0..count).map(|_| AtomicU32::new(0)));
    let next = AtomicUsize::new(0);
    let failed = thread::scope(|scope| {
        let handles: Vec<_> = (0..threads)
            .map(|_| {
                scope.spawn(|| -> Result<(), WeldError> {
                    let mut map: HashMap<Key, u32, Seeded> = HashMap::with_hasher(seeded);
                    loop {
                        let shard = next.fetch_add(1, Ordering::Relaxed);
                        if shard >= SHARDS {
                            return Ok(());
                        }
                        map.clear();
                        for chunk in &lists {
                            for &corner in &chunk[shard] {
                                map.try_reserve(1).map_err(|_| WeldError::Allocation)?;
                                let key = Key(point(corner as usize).key());
                                let representative = *map.entry(key).or_insert(corner);
                                first[corner as usize].store(representative, Ordering::Relaxed);
                            }
                        }
                    }
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().unwrap_or(Err(WeldError::Allocation)))
            .find(Result::is_err)
    });
    if let Some(error) = failed {
        return error.map(|()| Vec::new());
    }
    drop(lists);
    Ok(first.into_iter().map(AtomicU32::into_inner).collect())
}

/// Number first corners in order of occurrence and map every corner to its vertex, in place.
fn number(
    mut first: Vec<u32>,
    point: &(dyn Fn(usize) -> Point3 + Sync),
    max_vertices: usize,
) -> Result<Welded, WeldError> {
    let unique = first
        .iter()
        .enumerate()
        .filter(|&(corner, &f)| f as usize == corner)
        .count();
    if unique > max_vertices {
        return Err(WeldError::TooManyVertices);
    }
    let mut vertices = Vec::new();
    reserve(&mut vertices, unique)?;
    for corner in 0..first.len() {
        let representative = first[corner] as usize;
        if representative == corner {
            // Fits u32: at most `count` vertices, checked by `weld`.
            first[corner] = u32::try_from(vertices.len()).map_err(|_| WeldError::Allocation)?;
            vertices.push(point(corner));
        } else {
            // The representative is an earlier corner, already holding its vertex index.
            first[corner] = first[representative];
        }
    }
    Ok(Welded {
        vertices,
        corner_vertex: first,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn points(n: usize, distinct: u64, seed: u64) -> Vec<Point3> {
        let mut state = seed | 1;
        (0..n)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                #[allow(clippy::cast_precision_loss)] // Small test values.
                let v = (state % distinct) as f64;
                // Signed zeros must weld together.
                if v == 0.0 && state & 1 == 0 {
                    Point3::new(-0.0, 0.0, -0.0)
                } else {
                    Point3::new(v, v * 0.5, -v)
                }
            })
            .collect()
    }

    #[test]
    fn parallel_welding_equals_first_occurrence_numbering() {
        for (n, distinct) in [
            (0, 1),
            (5, 3),
            (1 << 17, 1000),
            (1 << 19, 1 << 18),
            (300_001, 7),
        ] {
            let corners = points(n, distinct, n as u64 + distinct);
            let point = |c: usize| corners[c];
            let welded = weld(n, &point, usize::MAX).unwrap();
            // Reference: a sequential first-occurrence map.
            let mut reference: HashMap<[u64; 3], u32> = HashMap::new();
            let mut vertices = Vec::new();
            let mut corner_vertex = Vec::new();
            for p in &corners {
                let next = u32::try_from(vertices.len()).unwrap();
                let id = *reference.entry(p.key()).or_insert_with(|| {
                    vertices.push(*p);
                    next
                });
                corner_vertex.push(id);
            }
            assert_eq!(welded.corner_vertex, corner_vertex, "n {n}");
            assert_eq!(welded.vertices.len(), vertices.len());
            for (a, b) in welded.vertices.iter().zip(&vertices) {
                assert_eq!(a.key(), b.key());
            }
            if !vertices.is_empty() {
                assert_eq!(
                    weld(n, &point, vertices.len() - 1).err(),
                    Some(WeldError::TooManyVertices)
                );
            }
        }
    }
}
