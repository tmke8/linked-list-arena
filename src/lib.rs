use std::cell::RefCell;
use std::marker::PhantomPinned;
use std::mem::MaybeUninit;
use std::pin::Pin;
use std::ptr::NonNull;

pub mod double;

pub struct Arena<const N: usize, T> {
    inner: RefCell<Option<InnerArena<N, T>>>,
}

struct InnerArena<const N: usize, T> {
    /// A link to the first element of a linked list of arena chunks.
    head_chunk: Link<N, T>,
    /// A pointer to the next object to be allocated.
    ptr: NonNull<MaybeUninit<T>>,
    /// A pointer to the end of the current chunk.
    ///
    /// You might wonder, why we cannot just use `chunks.slots.as_ptr().add(N)` to get
    /// the end of the chunk. The reason is that MIRI then complains because of something
    /// to do with tagged pointers.
    end: NonNull<MaybeUninit<T>>,
}

type Link<const N: usize, T> = Pin<Box<Chunk<N, T>>>;

struct Chunk<const N: usize, T> {
    slots: [MaybeUninit<T>; N],
    next: Option<Link<N, T>>,
    _pin: PhantomPinned,
}

impl<const N: usize, T> Arena<N, T> {
    /// Creates a new arena.
    /// This function does not allocate any memory.
    pub fn new() -> Self {
        assert!(std::mem::size_of::<T>() != 0);
        Arena {
            inner: RefCell::new(None),
        }
    }

    /// Allocates a new element in the arena and returns a mutable reference to it.
    #[allow(clippy::mut_from_ref)]
    pub fn alloc(&self, elem: T) -> &mut T {
        // Check whether anything has been allocated yet.
        if let Some(arena) = self.inner.borrow_mut().as_mut() {
            let mut ptr = arena.ptr;
            // Check whether there is still space in the current chunk.
            if ptr < arena.end {
                let slot = unsafe {
                    // Advance the pointer and turn the pointer into a mutable reference.
                    arena.ptr = ptr.add(1);
                    ptr.as_mut()
                };
                return slot.write(elem);
            }
        }

        // We either haven't allocated anything yet or the current chunk is full.
        // Both mean we have to allocate a new chunk.
        let old_head = self.inner.take().map(|a| a.head_chunk);
        let mut new_chunk = Box::into_pin(Box::new(Chunk {
            slots: [const { MaybeUninit::uninit() }; N],
            // The link to the previous head is stored in the new chunk.
            next: old_head,
            _pin: PhantomPinned,
        }));

        let slot = unsafe {
            // Get a mutable reference to the new chunk.
            // We have to be careful here, because the chunks are pinned, so we may
            // not use the mutable reference to move the chunk in memory.
            let new_chunk_mut = new_chunk.as_mut().get_unchecked_mut();
            // Get a pointer to the first slot in the new chunk.
            let mut ptr = NonNull::new_unchecked(new_chunk_mut.slots.as_mut_ptr());
            // We store the link to the new chunk in the arena.
            self.inner.replace(Some(InnerArena {
                head_chunk: new_chunk,
                ptr: ptr.add(1),
                end: ptr.add(N),
            }));
            ptr.as_mut()
        };
        slot.write(elem)
    }

    pub fn is_empty(&self) -> bool {
        self.inner.borrow().is_none()
    }

    /// Returns the number of free slots in the current chunk.
    /// If no chunk has been allocated yet, `None` is returned.
    pub fn free_slots_in_current_chunk(&self) -> Option<usize> {
        self.inner
            .borrow()
            .as_ref()
            .map(|arena| unsafe { arena.end.offset_from(arena.ptr) as usize })
    }

    /// Consumes the arena and destroys it.
    ///
    /// This is potentially more efficient than relying on the default Drop implementation,
    /// but it has the disadvantage that it cannot be used if there are internal references
    /// between the elements in the arena.
    ///
    /// This also calls the destructor of all elements in the arena.
    pub fn destroy(self) {
        if let Some(arena) = self.inner.into_inner() {
            unsafe {
                let mut head_chunk = Pin::into_inner_unchecked(arena.head_chunk);
                // Iterate over the elements in `head_chunk.slots` until `arena.ptr`
                // and call `assume_init_drop()` on each of them, because we know that they
                // have been initialized.
                let mut ptr = NonNull::new_unchecked(head_chunk.slots.as_mut_ptr());
                while ptr < arena.ptr {
                    ptr.as_mut().assume_init_drop();
                    ptr = ptr.add(1);
                }

                // Iterate over the linked list of chunks and drop all elements.
                let mut cur_link = head_chunk.next.take();
                while let Some(boxed_node) = cur_link {
                    let mut chunk = Pin::into_inner_unchecked(boxed_node);
                    // In the chunks that are not the head chunk, all elements have been initialized.
                    chunk.slots.iter_mut().for_each(|slot| {
                        slot.assume_init_drop();
                    });
                    cur_link = chunk.next.take();
                }
            }
        }
    }
}

impl<const N: usize, T> Default for Arena<N, T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod test {
    use std::cell::Cell;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use super::*;

    #[test]
    fn empty_arena() {
        let arena = Arena::<10, i32>::new();
        assert!(arena.is_empty());
    }

    #[test]
    fn free_slots() {
        let arena = Arena::<10, i32>::new();
        arena.alloc(1);
        assert!(!arena.is_empty());
        assert_eq!(arena.free_slots_in_current_chunk(), Some(9));
    }

    #[test]
    fn deref_allocated_elements() {
        let arena = Arena::<10, i32>::new();
        let el1 = arena.alloc(2);
        let el2 = arena.alloc(3);
        assert_eq!(*el1, 2);
        assert_eq!(*el2, 3);
    }

    #[test]
    fn fill_chunk() {
        let arena = Arena::<3, i32>::new();

        arena.alloc(1);
        assert_eq!(arena.free_slots_in_current_chunk(), Some(2));
        let el1 = arena.alloc(2);
        assert_eq!(arena.free_slots_in_current_chunk(), Some(1));
        arena.alloc(3);
        assert_eq!(arena.free_slots_in_current_chunk(), Some(0));
        let el2 = arena.alloc(4);
        assert_eq!(arena.free_slots_in_current_chunk(), Some(2));
        arena.alloc(5);
        assert_eq!(arena.free_slots_in_current_chunk(), Some(1));
        assert_eq!(*el1, 2);
        assert_eq!(*el2, 4);
        *el2 = 6;
        assert_eq!(*el1, 2);
        assert_eq!(*el2, 6);
        arena.alloc(6);
        assert_eq!(arena.free_slots_in_current_chunk(), Some(0));
        let el3 = arena.alloc(7);
        assert_eq!(arena.free_slots_in_current_chunk(), Some(2));
        assert_eq!(*el2, 6);
        assert_eq!(*el3, 7);
        arena.destroy();
    }

    #[cfg(target_pointer_width = "64")]
    #[test]
    fn data_structure_size() {
        assert_eq!(std::mem::size_of::<usize>(), 8);
        assert_eq!(std::mem::size_of::<InnerArena<1, i32>>(), 24);
        assert_eq!(std::mem::size_of::<Arena<1, i32>>(), 32);
        assert_eq!(std::mem::size_of::<Chunk<100, i32>>(), 408);
    }

    struct CycleParticipant<'a> {
        other: Cell<Option<&'a CycleParticipant<'a>>>,
    }

    #[test]
    fn cycle() {
        let arena: Arena<10, _> = Arena::new();

        let a = arena.alloc(CycleParticipant {
            other: Cell::new(None),
        });
        let b = arena.alloc(CycleParticipant {
            other: Cell::new(None),
        });

        a.other.set(Some(b));
        b.other.set(Some(a));
    }

    struct WithDrop(i32, Arc<AtomicUsize>);

    impl Drop for WithDrop {
        fn drop(&mut self) {
            self.1.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn drop_arena() {
        let drop_counter = Arc::new(AtomicUsize::new(0));

        let arena = Arena::<3, WithDrop>::new();
        let el1 = arena.alloc(WithDrop(1, Arc::clone(&drop_counter)));
        arena.alloc(WithDrop(2, Arc::clone(&drop_counter)));
        arena.alloc(WithDrop(3, Arc::clone(&drop_counter)));
        arena.alloc(WithDrop(4, Arc::clone(&drop_counter)));
        arena.alloc(WithDrop(5, Arc::clone(&drop_counter)));
        arena.alloc(WithDrop(6, Arc::clone(&drop_counter)));
        arena.alloc(WithDrop(7, Arc::clone(&drop_counter)));
        assert_eq!(el1.0, 1);
        arena.destroy(); // Should be calling drop on all elements.

        assert_eq!(drop_counter.load(Ordering::SeqCst), 7);
    }
}
