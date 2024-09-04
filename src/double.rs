// use std::array;
use std::cell::{Cell, RefCell};
// use std::collections::linked_list;
use std::collections::LinkedList;
use std::mem::MaybeUninit;
use std::ptr::NonNull;

pub struct DoublyLinkedArena<const N: usize, T> {
    list: RefCell<LinkedList<[MaybeUninit<T>; N]>>,
    /// A pointer to the next object to be allocated.
    ptr: Cell<Option<NonNull<MaybeUninit<T>>>>,
    /// A pointer to the end of the current chunk.
    end: Cell<Option<NonNull<MaybeUninit<T>>>>,
}

impl<const N: usize, T> DoublyLinkedArena<N, T> {
    /// Creates a new arena.
    /// This function does not allocate any memory.
    pub fn new() -> Self {
        assert!(std::mem::size_of::<T>() != 0);
        assert!(N != 0);
        DoublyLinkedArena {
            list: RefCell::new(LinkedList::new()),
            ptr: Cell::new(None),
            end: Cell::new(None),
        }
    }

    /// Allocates a new element in the arena and returns a mutable reference to it.
    #[allow(clippy::mut_from_ref)]
    pub fn alloc(&self, elem: T) -> &mut T {
        // Check whether anything has been allocated yet.
        if let Some(mut ptr) = self.ptr.get() {
            let end = self.end.get().unwrap();
            // Check whether there is still space in the current chunk.
            if ptr < end {
                let slot = unsafe {
                    // Advance the pointer and turn the pointer into a mutable reference.
                    self.ptr.set(Some(ptr.add(1)));
                    ptr.as_mut()
                };
                return slot.write(elem);
            }
        }
        // Allocate a new chunk.
        let mut list = self.list.borrow_mut();
        list.push_back([const { MaybeUninit::uninit() }; N]);
        unsafe {
            let ptr = NonNull::new_unchecked(list.back_mut().unwrap().as_mut_ptr());
            self.ptr.set(Some(ptr));
            self.end.set(ptr.add(N).into());
        }
        // Recurse to allocate the element in the new chunk.
        self.alloc(elem)
    }

    pub fn is_empty(&self) -> bool {
        self.list.borrow().is_empty()
    }

    /// Returns the number of free slots in the current chunk.
    /// If no chunk has been allocated yet, `None` is returned.
    pub fn free_slots_in_current_chunk(&self) -> Option<usize> {
        self.end
            .get()
            .map(|end| unsafe { end.offset_from(self.ptr.get().unwrap()) as usize })
    }

    // pub fn into_iter(self) -> IntoIter<N, T> {
    //     IntoIter {
    //         list_iter: self.list.into_inner().into_iter(),
    //         chunk_iter: None,
    //         ptr: self.ptr.get(),
    //     }
    // }
}

// pub struct IntoIter<const N: usize, T> {
//     list_iter: linked_list::IntoIter<[MaybeUninit<T>; N]>,
//     chunk_iter: Option<array::IntoIter<MaybeUninit<T>, N>>,
//     ptr: Option<NonNull<MaybeUninit<T>>>,
// }

// impl<const N: usize, T> Iterator for IntoIter<N, T> {
//     type Item = T;

//     fn next(&mut self) -> Option<Self::Item> {
//         if let Some(mut chunk_iter) = &mut self.chunk_iter {
//             if let Some(slot) = chunk_iter.next() {
//                 return Some(unsafe { slot.assume_init() });
//             }
//         }
//         todo!()
//     }
// }

#[cfg(test)]
mod test {
    use std::cell::Cell;

    use super::*;

    #[test]
    fn empty_arena() {
        let arena = DoublyLinkedArena::<10, i32>::new();
        assert!(arena.is_empty());
    }

    #[test]
    fn free_slots() {
        let arena = DoublyLinkedArena::<10, i32>::new();
        arena.alloc(1);
        assert!(!arena.is_empty());
        assert_eq!(arena.free_slots_in_current_chunk(), Some(9));
    }

    #[test]
    fn deref_allocated_elements() {
        let arena = DoublyLinkedArena::<10, i32>::new();
        let el1 = arena.alloc(2);
        let el2 = arena.alloc(3);
        assert_eq!(*el1, 2);
        assert_eq!(*el2, 3);
    }

    #[test]
    fn fill_chunk() {
        let arena = DoublyLinkedArena::<3, i32>::new();

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
    fn data_structure_size() {
        assert_eq!(std::mem::size_of::<usize>(), 8);
        assert_eq!(std::mem::size_of::<DoublyLinkedArena<1, i32>>(), 48);
    }

    struct CycleParticipant<'a> {
        other: Cell<Option<&'a CycleParticipant<'a>>>,
    }

    #[test]
    fn cycle() {
        let arena: DoublyLinkedArena<10, _> = DoublyLinkedArena::new();

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
