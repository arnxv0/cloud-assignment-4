use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct CreateOrderRequest {
    pub customer_id: String,
    pub item_id: String,
    pub quantity: i32,
}

#[derive(Debug, Deserialize, Clone)]
pub struct CreateItemRequest {
    pub name: String,
    pub value: i32,
}
