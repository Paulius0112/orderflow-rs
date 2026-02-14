use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Buy,
    Sell,
}

impl fmt::Display for Side {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Side::Buy => write!(f, "BUY"),
            Side::Sell => write!(f, "SELL"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderType {
    Limit,
    Market,
}

impl fmt::Display for OrderType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OrderType::Limit => write!(f, "LIMIT"),
            OrderType::Market => write!(f, "MARKET"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Order {
    pub id: u64,
    pub side: Side,
    pub order_type: OrderType,
    pub price: f64,
    pub size: u32,
    pub created_at: f64,
    pub ttl: f64,
}

impl Order {
    pub fn to_wire(&self) -> String {
        format!(
            "ORDER|id={}|side={}|type={}|price={:.2}|size={}|time={:.3}",
            self.id, self.side, self.order_type, self.price, self.size, self.created_at,
        )
    }
}

pub fn cancel_to_wire(order_id: u64, current_time: f64) -> String {
    format!("CANCEL|id={}|time={:.3}", order_id, current_time)
}
