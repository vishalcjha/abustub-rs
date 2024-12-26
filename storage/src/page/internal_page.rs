#![allow(dead_code)]

use std::{slice, usize};

use common::{GenericKey, PageId, PAGE_SIZE};

use super::page_header::{PageHeader, PAGE_HEADER_SIZE};
pub struct InternalPageEntry<'a, const N: usize>(&'a GenericKey<N>, &'a PageId);
trait LeafPage {}
const fn get_element_count<const N: usize>() -> usize {
    (PAGE_SIZE - PAGE_HEADER_SIZE)
        / (std::mem::size_of::<GenericKey<N>>() + std::mem::size_of::<PageId>())
}

#[derive(Debug)]
struct ReadOnlyInternalPage<'a, const N: usize> {
    page_header: PageHeader,
    keys: &'a [GenericKey<N>],
    page_ids: &'a [PageId],
}

impl<'a, const N: usize> From<&[u8]> for ReadOnlyInternalPage<'a, N> {
    fn from(data: &[u8]) -> Self {
        let data: &[u8; PAGE_SIZE] = data.try_into().unwrap();
        let page_header = PageHeader::new(&data, get_element_count::<N>() as u16);
        let mut offset = PAGE_HEADER_SIZE;
        let element_count = page_header.get_max_size() as usize;
        let keys = GenericKey::<N>::from_raw_parts(&data, offset, element_count);
        offset += std::mem::size_of::<GenericKey<N>>() * element_count;

        let page_ids = unsafe {
            let mut start_of_keys = data.as_ptr().add(offset);
            let offset = start_of_keys.align_offset(align_of::<PageId>());
            start_of_keys = start_of_keys.add(offset);
            slice::from_raw_parts::<PageId>(start_of_keys as *const PageId, element_count)
        };

        Self {
            page_header,
            keys,
            page_ids,
        }
    }
}

impl<const N: usize> ReadOnlyInternalPage<'_, N> {
    pub fn get_entry_at_pos(&self, pos: usize) -> InternalPageEntry<N> {
        assert!(pos < self.page_header.get_current_size() as usize);

        InternalPageEntry::<N>(&self.keys[pos], &self.page_ids[pos])
    }
}

#[derive(Debug)]
struct MutableInternalPage<'a, const N: usize> {
    data: &'a mut Box<[u8; 4096]>,
    page_header: PageHeader,
    keys: &'a mut [GenericKey<N>],
    page_ids: &'a mut [PageId],
}

impl<'a, const N: usize> From<&'a mut Box<[u8; PAGE_SIZE]>> for MutableInternalPage<'a, N> {
    fn from(data: &'a mut Box<[u8; PAGE_SIZE]>) -> Self {
        let page_header = PageHeader::new(data, get_element_count::<N>() as u16);
        let mut offset = PAGE_HEADER_SIZE;
        let element_count = get_element_count::<N>();

        let keys = GenericKey::<N>::from_raw_parts_mut(data, offset, element_count);
        offset += std::mem::size_of::<GenericKey<N>>() * element_count;

        let page_ids = unsafe {
            let mut start_of_keys = data.as_mut_ptr().add(offset);
            let offset = start_of_keys.align_offset(align_of::<PageId>());
            start_of_keys = start_of_keys.add(offset);
            slice::from_raw_parts_mut::<PageId>(start_of_keys as *mut PageId, element_count)
        };

        Self {
            data,
            page_header,
            keys,
            page_ids,
        }
    }
}

/// Drop in MutableLeafPage actually writes page header data back to source memory.
impl<'a, const N: usize> Drop for MutableInternalPage<'a, N> {
    fn drop(&mut self) {
        self.page_header.serialize(self.data);
    }
}

impl<const N: usize> MutableInternalPage<'_, N> {
    fn add_entry(&mut self, key: impl Into<GenericKey<N>>, page_id: PageId) {
        assert!(self.page_header.get_current_size() < self.page_header.get_max_size());
        let index_to_add: usize = self.page_header.get_current_size() as usize;
        self.set_at_pos(index_to_add, key, page_id);

        self.page_header.set_current_size(index_to_add as u16 + 1);
    }

    fn set_at_pos(&mut self, pos: usize, key: impl Into<GenericKey<N>>, page_id: PageId) {
        assert!(pos < self.page_header.get_max_size() as usize);
        self.keys[pos] = key.into();
        self.page_ids[pos] = page_id;
    }

    fn get_entry(&mut self, pos: usize) -> InternalPageEntry<N> {
        InternalPageEntry(&self.keys[pos], &self.page_ids[pos])
    }
}

#[cfg(test)]
mod test {

    use common::PAGE_SIZE;

    use crate::page::internal_page::get_element_count;

    use super::{MutableInternalPage, ReadOnlyInternalPage};

    #[test]
    fn test_element_count() {
        println!("{}", get_element_count::<1>());
        println!("{}", get_element_count::<8>());
        println!("{}", get_element_count::<10>());
        println!("{}", get_element_count::<16>());
        println!("{}", get_element_count::<32>());
        println!("{}", get_element_count::<64>());
    }

    #[test]
    fn test_internal_page_creation() {
        let mut data = Box::new([0; PAGE_SIZE]);

        let mut writable = MutableInternalPage::<4>::from(&mut data);

        writable.add_entry(16, 20);
        writable.add_entry(20, 131);
        drop(writable);

        let read_only = ReadOnlyInternalPage::<4>::from(data.as_slice());
        assert_eq!(read_only.page_header.get_current_size(), 2);
        let first_entry = read_only.get_entry_at_pos(0);
        assert_eq!(Into::<u32>::into(first_entry.0.clone()), 16);
        assert_eq!(*first_entry.1, 20);

        let second_entry = read_only.get_entry_at_pos(1);
        assert_eq!(Into::<u32>::into(second_entry.0.clone()), 20);
        assert_eq!(*second_entry.1, 131);
    }
}
