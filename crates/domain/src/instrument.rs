//! The instrument specification — the security master's row.
//!
//! What a venue publishes about a symbol before anyone trades it: the price
//! grid it accepts (tick), the quantity grid (lot), and any size cap. An empty
//! registry means "unrestricted": any well-formed symbol trades on the finest
//! grid the system has (tick 0.0001, lot 1).

use crate::{DomainError, Px, Qty, Symbol};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstrumentSpec {
    pub symbol: Symbol,
    /// Minimum price increment. An order's limit must sit on this grid —
    /// tick 0.0100 means $10.0050 is REJECTED, exactly as Reg NMS would.
    /// The finest legal tick is the system's own grid, 0.0001.
    pub tick: Px,
    /// Quantity step. Orders must be whole multiples; lot 1 = any whole size.
    pub lot: Qty,
    /// Per-order size cap, if any.
    pub max_order_qty: Option<Qty>,
}

impl InstrumentSpec {
    pub fn new(
        symbol: Symbol,
        tick: Px,
        lot: Qty,
        max_order_qty: Option<Qty>,
    ) -> Result<Self, DomainError> {
        if lot.is_zero() {
            return Err(DomainError::NonPositiveQty(0));
        }
        Ok(Self {
            symbol,
            tick,
            lot,
            max_order_qty,
        })
    }

    /// The default spec for an unrestricted world: the system's own grids.
    pub const fn permissive(symbol: Symbol) -> Self {
        Self {
            symbol,
            tick: Px::MIN_TICK,
            lot: Qty::ONE,
            max_order_qty: None,
        }
    }

    /// Is this limit price on the instrument's grid?
    pub fn px_on_tick(&self, px: Px) -> bool {
        px.raw() % self.tick.raw() == 0
    }

    /// Is this quantity a whole number of lots?
    pub fn qty_on_lot(&self, qty: Qty) -> bool {
        qty.get() % self.lot.get() == 0
    }
}
