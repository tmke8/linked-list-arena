use std::cell::RefCell;
use std::marker::{PhantomData, PhantomPinned};
use std::mem::MaybeUninit;
use std::pin::Pin;
use std::ptr::NonNull;

pub struct Arena<const N: usize, T>(RefCell<Option<InnerArena<N, T>>>);

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
    /// Marker indicating that dropping the arena causes its owned
    /// instances of `T` to be dropped.
    _own: PhantomData<T>,
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
        Arena(RefCell::new(None))
    }

    /// Allocates a new element in the arena and returns a mutable reference to it.
    #[allow(clippy::mut_from_ref)]
    pub fn alloc(&self, elem: T) -> &mut T {
        // Check whether anything has been allocated yet.
        if let Some(arena) = self.0.borrow_mut().as_mut() {
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
        let old_head = self.0.take().map(|a| a.head_chunk);
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
            self.0.replace(Some(InnerArena {
                head_chunk: new_chunk,
                ptr: ptr.add(1),
                end: ptr.add(N),
                _own: PhantomData,
            }));
            ptr.as_mut()
        };
        slot.write(elem)
    }

    pub fn is_empty(&self) -> bool {
        self.0.borrow().is_none()
    }

    /// Returns the number of free slots in the current chunk.
    /// If no chunk has been allocated yet, `None` is returned.
    pub fn free_slots_in_current_chunk(&self) -> Option<usize> {
        self.0
            .borrow()
            .as_ref()
            .map(|arena| unsafe { arena.end.offset_from(arena.ptr) as usize })
    }
}

impl<const N: usize, T> Default for Arena<N, T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize, T> Drop for Arena<N, T> {
    fn drop(&mut self) {
        let mut cur_link = self.0.take().map(|s| s.head_chunk);
        while let Some(boxed_node) = cur_link {
            unsafe {
                cur_link = Pin::into_inner_unchecked(boxed_node).next.take();
            }
        }
    }
}

#[cfg(test)]
mod test {
    use std::cell::Cell;

    use super::*;

    #[test]
    fn empty_arena() {
        let arena = Arena::<10, i32>::new();

        // Check empty arena behaves right
        assert!(arena.is_empty());
    }

    #[test]
    fn free_slots() {
        let arena = Arena::<10, i32>::new();
        // Populate list
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
    }

    #[cfg(target_pointer_width = "64")]
    #[test]
    fn arena_size() {
        assert_eq!(std::mem::size_of::<usize>(), 8);
        assert_eq!(std::mem::size_of::<InnerArena<10, i32>>(), 24);
        assert_eq!(std::mem::size_of::<Arena<10, i32>>(), 32);
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
}
