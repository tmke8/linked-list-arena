use std::cell::{Cell, RefCell};
use std::marker::PhantomData;
use std::mem::MaybeUninit;
use std::pin::Pin;
use std::ptr::NonNull;

/// An arena allocator that is made up of fixed-sized chunks of memory.
///
/// Only pushing is supported.
pub struct Arena<const N: usize, T>{
    head: Cell<Link<N, T>>,
    tail: Cell<Link<N, T>>,
    current_index: Cell<usize>,
}

type Link<const N: usize, T> = Option<NonNull<Chunk<N, T>>>;

struct Chunk<const N: usize, T> {
    slots: [MaybeUninit<T>; N],
    next: Link<N, T>,
}

impl<const N: usize, T> Arena<N, T> {
    pub fn new() -> Self {
        Arena {
            head: Cell::new(None),
            tail: Cell::new(None),
            current_index: Cell::new(0),
        }
    }
    pub fn push(&self, elem: T) -> &mut T {
        if let Some(tail) = self.tail.get() {
            let current_index = self.current_index.get();
            unsafe {
            if let Some(slot) = (*tail.as_ptr()).slots.get_mut(current_index) {
                self.current_index.set(current_index + 1);
                let ptr = slot.as_mut_ptr();
                *ptr = elem;
                return &mut *ptr;
            }}
        }
        // Allocate a new chunk.
        unsafe {
            let new_tail = Box::into_raw(Box::new(Chunk {
                slots: [const { MaybeUninit::uninit() }; N],
                next: None,
            }));
            let new_tail = NonNull::new(new_tail);

            if let Some(tail) = self.tail.get() {
                (*tail.as_ptr()).next = new_tail;
            } else {
                self.head.set(new_tail);
            }

            self.tail.set(new_tail);
            self.current_index.set(1);
            let ptr = (*self.tail.get().unwrap().as_ptr()).slots[0].as_mut_ptr();
            *ptr = elem;
            &mut *ptr
        }
    }
    pub fn is_empty(&self) -> bool {
        self.head.get().is_none()
    }
    fn free_slots_in_current_chunk(&self) -> usize {
        N - self.current_index.get()
    }
}

impl<const N: usize, T> Drop for Arena<N, T> {
    fn drop(&mut self) {
        let mut head = self.head.get();
        while let Some(chunk) = head {
            unsafe {
                // Reconstruct Box so we can drop it
                let boxed_chunk = Box::from_raw(chunk.as_ptr());
                head = boxed_chunk.next;
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
        arena.push(1);
        assert!(!arena.is_empty());
        assert_eq!(arena.free_slots_in_current_chunk(), 9);

        let arena = Arena::<10, i32>::new();
        let el1 = arena.push(2);
        let el2 = arena.push(3);
        assert_eq!(*el1, 2);
        assert_eq!(*el2, 3);
        /* *el1 = 4;
        assert_eq!(*el1, 4); */
    }

    /* #[test]
    fn miri_food() {
        let mut list = List::<10, i32>::new();

        list.push(1);
        list.push(2);
        list.push(3);

        assert!(list.pop() == Some(1));
        list.push(4);
        assert!(list.pop() == Some(2));
        list.push(5);

        /* assert!(list.peek() == Some(&3));
        list.push(6);
        list.peek_mut().map(|x| *x *= 10);
        assert!(list.peek() == Some(&30));
        assert!(list.pop() == Some(30));

        for elem in list.iter_mut() {
            *elem *= 100;
        }

        let mut iter = list.iter();
        assert_eq!(iter.next(), Some(&400));
        assert_eq!(iter.next(), Some(&500));
        assert_eq!(iter.next(), Some(&600));
        assert_eq!(iter.next(), None);
        assert_eq!(iter.next(), None);

        assert!(list.pop() == Some(400));
        list.peek_mut().map(|x| *x *= 10);
        assert!(list.peek() == Some(&5000));
        list.push(7); */

        // Drop it on the ground and let the dtor exercise itself
    } */
}
