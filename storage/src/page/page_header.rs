#![allow(dead_code)]

#[repr(C)]
pub struct PageHeader {
    current_size: u16,
    max_size: u16,
}

#[cfg(test)]
mod test {
    use crate::page::page_header::PageHeader;

    #[test]
    fn get_page_type_size() {
        println!("{}", std::mem::size_of::<PageHeader>());
        println!("{}", std::mem::size_of::<u16>());
    }
}
