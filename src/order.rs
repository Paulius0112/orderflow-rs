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
    pub fn to_wire_text(&self) -> String {
        format!(
            "ORDER|id={}|side={}|type={}|price={:.2}|size={}|time={:.3}",
            self.id, self.side, self.order_type, self.price, self.size, self.created_at,
        )
    }

    /// Binary wire format (v1), little-endian:
    /// magic[2]="OF", version:u8=1, msg_type:u8=1 (order),
    /// id:u64, side:u8 (1 buy, 2 sell), order_type:u8 (1 limit, 2 market),
    /// price:f64, size:u32, time:f64
    pub fn to_wire_binary(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(2 + 1 + 1 + 8 + 1 + 1 + 8 + 4 + 8);
        out.extend_from_slice(b"OF");
        out.push(1);
        out.push(1);
        out.extend_from_slice(&self.id.to_le_bytes());
        out.push(match self.side {
            Side::Buy => 1,
            Side::Sell => 2,
        });
        out.push(match self.order_type {
            OrderType::Limit => 1,
            OrderType::Market => 2,
        });
        out.extend_from_slice(&self.price.to_le_bytes());
        out.extend_from_slice(&self.size.to_le_bytes());
        out.extend_from_slice(&self.created_at.to_le_bytes());
        out
    }
}

pub fn cancel_to_wire_text(order_id: u64, current_time: f64) -> String {
    format!("CANCEL|id={}|time={:.3}", order_id, current_time)
}

/// Binary cancel wire format (v1), little-endian:
/// magic[2]="OF", version:u8=1, msg_type:u8=2 (cancel), id:u64, time:f64
pub fn cancel_to_wire_binary(order_id: u64, current_time: f64) -> Vec<u8> {
    let mut out = Vec::with_capacity(2 + 1 + 1 + 8 + 8);
    out.extend_from_slice(b"OF");
    out.push(1);
    out.push(2);
    out.extend_from_slice(&order_id.to_le_bytes());
    out.extend_from_slice(&current_time.to_le_bytes());
    out
}
