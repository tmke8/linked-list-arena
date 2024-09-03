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
        if let Some(inner) = self.0.borrow_mut().as_mut() {
            let mut ptr = inner.ptr;
            let end = inner.end;
            unsafe {
                if ptr < end {
                    inner.ptr = ptr.add(1);
                    return ptr.as_mut().write(elem);
                }
            }
        }
        // Allocate a new chunk.
        let old_head = self.0.take().map(|s| s.chunks);
        let new_chunk = Box::new(Chunk {
            slots: [const { MaybeUninit::uninit() }; N],
            next: old_head,
            _pin: PhantomPinned,
        });

        self.0.replace(Some(InnerArena {
            chunks: Box::into_pin(new_chunk),
            ptr: NonNull::dangling(),
            end: NonNull::dangling(),
        }));
        let mut inner = self.0.borrow_mut();
        unsafe {
            let inner = inner.as_mut().unwrap();
            let mut ptr = NonNull::new_unchecked(
                inner.chunks.as_mut().get_unchecked_mut().slots.as_mut_ptr(),
            );
            let end = ptr.add(N);
            inner.ptr = ptr.add(1);
            inner.end = end;
            ptr.as_mut().write(elem)
        }
    }
    pub fn is_empty(&self) -> bool {
        self.0.borrow().is_none()
    }
    pub fn free_slots_in_current_chunk(&self) -> usize {
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
    fn basics() {
        let arena = Arena::<10, i32>::new();

        // Check empty list behaves right
        assert!(arena.is_empty());

        let arena = Arena::<10, i32>::new();
        // Populate list
        arena.alloc(1);
        assert!(!arena.is_empty());
        assert_eq!(arena.free_slots_in_current_chunk(), 9);

        let arena = Arena::<10, i32>::new();
        let el1 = arena.alloc(2);
        let el2 = arena.alloc(3);
        assert_eq!(*el1, 2);
        assert_eq!(*el2, 3);
        /* *el1 = 4;
        assert_eq!(*el1, 4); */
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

    /* #[test]
    fn peek() {
        let mut list = List::new();
        assert_eq!(list.peek(), None);
        assert_eq!(list.peek_mut(), None);
        list.push(1);
        list.push(2);
        list.push(3);

        assert_eq!(list.peek(), Some(&3));
        assert_eq!(list.peek_mut(), Some(&mut 3));

        list.peek_mut().map(|value| *value = 42);

        assert_eq!(list.peek(), Some(&42));
        assert_eq!(list.pop(), Some(42));
    }

    #[test]
    fn into_iter() {
        let mut list = List::new();
        list.push(1);
        list.push(2);
        list.push(3);

        let mut iter = list.into_iter();
        assert_eq!(iter.next(), Some(3));
        assert_eq!(iter.next(), Some(2));
        assert_eq!(iter.next(), Some(1));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn iter() {
        let mut list = List::new();
        list.push(1);
        list.push(2);
        list.push(3);

        let mut iter = list.iter();
        assert_eq!(iter.next(), Some(&3));
        assert_eq!(iter.next(), Some(&2));
        assert_eq!(iter.next(), Some(&1));
    }

    #[test]
    fn iter_mut() {
        let mut list = List::new();
        list.push(1);
        list.push(2);
        list.push(3);

        let mut iter = list.iter_mut();
        assert_eq!(iter.next(), Some(&mut 3));
        assert_eq!(iter.next(), Some(&mut 2));
        assert_eq!(iter.next(), Some(&mut 1));
    } */
}
