#![allow(dead_code)]

use async_trait::async_trait;

#[async_trait]
trait PageOperator: Send {
    async fn increase_page_size(page_count: u32);
}
