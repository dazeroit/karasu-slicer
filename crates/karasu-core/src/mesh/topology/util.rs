use super::{MeshError, TopologyError};
use crate::cancellation::{Cancellation, Checkpoint};

pub(super) fn all<T>(
    items: &[T],
    mut predicate: impl FnMut(&T) -> bool,
    cancellation: &mut Cancellation<'_>,
    checkpoint: Checkpoint,
) -> Result<bool, TopologyError> {
    for (index, item) in items.iter().enumerate() {
        cancellation.poll(checkpoint, index)?;
        if !predicate(item) {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(super) fn count<T>(
    items: &[T],
    mut predicate: impl FnMut(&T) -> bool,
    cancellation: &mut Cancellation<'_>,
    checkpoint: Checkpoint,
) -> Result<usize, TopologyError> {
    let mut count = 0;
    for (index, item) in items.iter().enumerate() {
        cancellation.poll(checkpoint, index)?;
        count += usize::from(predicate(item));
    }
    Ok(count)
}

pub(super) fn reserve<T>(items: &mut Vec<T>, extra: usize) -> Result<(), TopologyError> {
    items
        .len()
        .checked_add(extra)
        .and_then(|n| n.checked_mul(size_of::<T>()))
        .filter(|&bytes| isize::try_from(bytes).is_ok())
        .ok_or(TopologyError::SizeOverflow)?;
    items
        .try_reserve(extra)
        .map_err(|_| MeshError::Allocation {
            context: "mesh topology buffers",
        })?;
    Ok(())
}

pub(super) fn push<T>(items: &mut Vec<T>, item: T) -> Result<(), TopologyError> {
    reserve(items, 1)?;
    items.push(item);
    Ok(())
}

pub(super) fn filled<T: Clone>(count: usize, value: T) -> Result<Vec<T>, TopologyError> {
    let mut items = Vec::new();
    reserve(&mut items, count)?;
    items.resize(count, value);
    Ok(items)
}

/// Iterative disjoint sets, union by size with deterministic tie breaking.
pub(super) struct UnionFind {
    parent: Vec<usize>,
    size: Vec<usize>,
}

impl UnionFind {
    pub(super) fn new(count: usize) -> Result<Self, TopologyError> {
        let mut parent = Vec::new();
        reserve(&mut parent, count)?;
        parent.extend(0..count);
        Ok(Self {
            parent,
            size: filled(count, 1)?,
        })
    }

    pub(super) fn root(&mut self, mut node: usize) -> usize {
        while self.parent[node] != node {
            self.parent[node] = self.parent[self.parent[node]];
            node = self.parent[node];
        }
        node
    }

    pub(super) fn join(&mut self, a: usize, b: usize) {
        let (mut a, mut b) = (self.root(a), self.root(b));
        if a == b {
            return;
        }
        if self.size[a] < self.size[b] || (self.size[a] == self.size[b] && a > b) {
            std::mem::swap(&mut a, &mut b);
        }
        self.parent[b] = a;
        // Both sets are disjoint subsets of the preallocated node buffer.
        self.size[a] += self.size[b];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn impossible_capacity_is_a_controlled_error() {
        assert_eq!(
            reserve(&mut Vec::<u64>::new(), usize::MAX),
            Err(TopologyError::SizeOverflow)
        );
    }
}
