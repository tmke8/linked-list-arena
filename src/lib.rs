use std::cell::RefCell;
use std::marker::PhantomPinned;
use std::mem::MaybeUninit;
use std::pin::Pin;
use std::ptr::NonNull;

pub struct Arena<const N: usize, T>(RefCell<Option<InnerArena<N, T>>>);

struct InnerArena<const N: usize, T> {
    /// A link to a linked list of arena chunks.
    chunks: Link<N, T>,
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
    pub fn new() -> Self {
        Arena(RefCell::new(None))
    }

    pub fn alloc(&self, elem: T) -> &mut T {
        // Check whether anything has been allocated yet.
        if let Some(inner) = self.0.borrow_mut().as_mut() {
            let mut ptr = inner.ptr;
            let end = inner.end;
            // Check whether there is still space in the current chunk.
            if ptr < end {
                unsafe {
                    inner.ptr = ptr.add(1);
                    return ptr.as_mut().write(elem);
                }
            }
        }

        // We either haven't allocated anything yet or the current chunk is full.
        // Both mean we have to allocate a new chunk.
        let old_head = self.0.take().map(|s| s.chunks);
        let new_chunk = Box::new(Chunk {
            slots: [const { MaybeUninit::uninit() }; N],
            // The link to the previous head is stored in the new chunk.
            next: old_head,
            _pin: PhantomPinned,
        });

        // We store the link to the new chunk in the arena.
        self.0.replace(Some(InnerArena {
            chunks: Box::into_pin(new_chunk),
            // We will initialize these pointers later.
            ptr: NonNull::dangling(),
            end: NonNull::dangling(),
        }));

        // Finally, we initialize the pointers and allocate the first object.
        // First, we get a mutable referenc to what we just wrote.
        let mut inner = self.0.borrow_mut();
        unsafe {
            let inner = inner.as_mut().unwrap_unchecked();
            // We get a mutable reference to the new chunk.
            // Note that the chunk is pinned, so we have to be careful to not induce
            // any moves.
            let chunk = inner.chunks.as_mut().get_unchecked_mut();
            // We get a pointer to the first element.
            let mut ptr = NonNull::new_unchecked(chunk.slots.as_mut_ptr());
            inner.ptr = ptr.add(1);
            inner.end = ptr.add(N);
            ptr.as_mut().write(elem)
        }
    }
    pub fn is_empty(&self) -> bool {
        self.0.borrow().is_none()
    }
    #[cfg(test)]
    fn free_slots_in_current_chunk(&self) -> usize {
        if let Some(inner) = self.0.borrow().as_ref() {
            return unsafe { inner.end.offset_from(inner.ptr) as usize };
        } else {
            N
        }
    }
}

impl<const N: usize, T> Drop for Arena<N, T> {
    fn drop(&mut self) {
        let mut cur_link = self.0.take().map(|s| s.chunks);
        while let Some(boxed_node) = cur_link {
            unsafe {
                cur_link = Pin::into_inner_unchecked(boxed_node).next.take();
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::Arena;

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
        assert_eq!(arena.free_slots_in_current_chunk(), 9);
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
        assert_eq!(arena.free_slots_in_current_chunk(), 2);
        let el1 = arena.alloc(2);
        assert_eq!(arena.free_slots_in_current_chunk(), 1);
        arena.alloc(3);
        assert_eq!(arena.free_slots_in_current_chunk(), 0);
        let el2 = arena.alloc(4);
        assert_eq!(arena.free_slots_in_current_chunk(), 2);
        arena.alloc(5);
        assert_eq!(arena.free_slots_in_current_chunk(), 1);
        assert_eq!(*el1, 2);
        assert_eq!(*el2, 4);
        *el2 = 6;
        assert_eq!(*el1, 2);
        assert_eq!(*el2, 6);
        arena.alloc(6);
        assert_eq!(arena.free_slots_in_current_chunk(), 0);
        let el3 = arena.alloc(7);
        assert_eq!(arena.free_slots_in_current_chunk(), 2);
        assert_eq!(*el2, 6);
        assert_eq!(*el3, 7);
    }
}
