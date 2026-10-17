//! The command dialogs of the original that take the place of the command
//! grid (Tzar.exe 1.11): D_GAMBLE (tavern `gamble`, 0x4a30b0), D_TRADE1
//! (tavern `trade1`, 0x4a5800), D_TRADE (trade routes, 0x4a1d30) and
//! D_build (magic workshop `build_item`, 0x4a4460).
//!
//! The controls have no labels: hovering one shows its `[orders_names]`
//! text in the hint line. Left click raises a number and right click lowers
//! it, Shift with the larger step. Positions are those of the side layouts,
//! relative to the dialog's top-left corner, (16,332) on the left and
//! (668 + width − 800, 332) on the right.
use crate::{gameplay::Request, menu::Rect};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Gamble,
    Exchange,
    Route,
    Workshop,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    Resource,
    Resource2,
    Amount,
    /// The exchange's received amount, shown only.
    Received,
    Odds,
    Market(u8),
    Category(u8),
    Item,
    Accept,
    Cancel,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Dialog {
    pub kind: Kind,
    /// The tavern, workshop or trader the dialog commands.
    pub object: usize,
    pub resource: u8,
    pub resource2: u8,
    /// Stake, offer, load per trip or gold.
    pub amount: u32,
    /// Gambling odds or workshop category.
    pub option: u8,
    pub item: u8,
    /// The two markets of a trade route.
    pub markets: [Option<usize>; 2],
    /// The market button waiting for a pick on the map (+0x30).
    pub picking: Option<u8>,
    /// `winlose.bmp` frame: 0 dice, 1 lost, 2 won.
    pub face: u8,
    /// The staked resource before a roll, until the result arrives.
    pub pending: Option<u32>,
    /// Milliseconds of the dice animation still to play.
    pub rolling: u32,
}
pub enum Outcome {
    Nothing,
    Close,
    /// An order, and whether the dialog closes with it.
    Send(Request, bool),
    Message(String),
}
/// `zarove.bmp`: 15 frames, about 70 ms each (`0x41b0f0(1, 70, 1, 1)`).
pub const DICE_FRAMES: u32 = 15;
pub const DICE_FRAME_MS: u32 = 70;
/// Resource icons of the table at 0x58dc8c (ICONS.INI keys).
pub const RESOURCE_ICONS: [&str; 4] = ["gold", "stone", "food", "wood"];
/// Frames of `ICONS\icons.bmp` for the workshop's items: books
/// (0x58dca4, 199 + spell), potions (0x58dcd4) and power items (0x58dccc).
pub fn item_icon(category: u8, item: u8) -> usize {
    match category {
        0 => 199 + item as usize,
        1 => [191, 192, 193, 194, 195][item.min(4) as usize],
        _ => [196, 197, 198][item.min(2) as usize],
    }
}
/// The pictures of the items the workshop makes (0x465ca0), whose
/// `[images]` names the hint shows.
pub fn item_image(category: u8, item: u8) -> &'static str {
    match (category, item) {
        (1, 0) => "item_potion2",
        (1, 1) => "item_potion3",
        (1, 2) => "item_potion1",
        (1, 3) => "item_potion7",
        (1, 4) => "item_potion6",
        (2, 0) => "item_sword1",
        (2, 1) => "item_bow1",
        _ => "item_shield1",
    }
}
/// Items per category (+0x35): twenty books (curse, the 21st spell, is
/// out of reach), five potions, three power items.
fn items(category: u8) -> u8 {
    [20, 5, 3][category.min(2) as usize]
}
const fn at(x: i32, y: i32, w: i32, h: i32) -> Rect {
    Rect { x, y, w, h }
}
impl Dialog {
    fn new(kind: Kind, object: usize) -> Self {
        Self {
            kind,
            object,
            resource: 0,
            resource2: 0,
            amount: 0,
            option: 0,
            item: 0,
            markets: [None; 2],
            picking: None,
            face: 0,
            pending: None,
            rolling: 0,
        }
    }
    /// D_GAMBLE starts at 50 of gold with the 1:1 odds.
    pub fn gamble(object: usize) -> Self {
        Self {
            amount: 50,
            ..Self::new(Kind::Gamble, object)
        }
    }
    /// D_TRADE1 starts from the tavern's last exchange, else 500.
    pub fn exchange(object: usize, last: Option<(u8, u8, u32)>) -> Self {
        let (from, to, offered) = last.unwrap_or((0, 0, 500));
        Self {
            resource: from,
            resource2: to,
            amount: offered.clamp(500, 20000),
            ..Self::new(Kind::Exchange, object)
        }
    }
    /// D_TRADE shows the unit's route, else a load of 50.
    pub fn route(
        object: usize,
        markets: [Option<usize>; 2],
        resources: [u8; 2],
        amount: Option<u32>,
    ) -> Self {
        Self {
            markets,
            resource: resources[0],
            resource2: resources[1],
            amount: amount.unwrap_or(50),
            ..Self::new(Kind::Route, object)
        }
    }
    /// D_build keeps the category and item between openings
    /// (0xc8aec8, 0xc8aec9); the gold starts at 100.
    pub fn workshop(object: usize, last: (u8, u8)) -> Self {
        Self {
            option: last.0.min(2),
            item: last.1.min(items(last.0) - 1),
            amount: 100,
            ..Self::new(Kind::Workshop, object)
        }
    }
    /// The controls with their hint keys, side layouts.
    pub fn controls(&self) -> Vec<(Control, Rect, &'static str)> {
        use Control::*;
        match self.kind {
            Kind::Gamble => vec![
                (Resource, at(8, 8, 48, 48), "gm_res"),
                (Amount, at(59, 8, 48, 48), "gm_amount"),
                (Odds, at(8, 62, 99, 48), "gm_prop"),
                (Cancel, at(8, 168, 48, 48), "gm_cancel"),
                (Accept, at(59, 168, 48, 48), "gm_gamble"),
            ],
            Kind::Exchange => vec![
                (Resource, at(8, 61, 48, 48), "tr1_srcres"),
                (Amount, at(59, 61, 48, 48), "tr1_amount1"),
                (Resource2, at(8, 114, 48, 48), "tr1_dstres"),
                (Received, at(59, 114, 48, 48), "tr1_amount2"),
                (Cancel, at(8, 168, 48, 48), "tr1_cancel"),
                (Accept, at(59, 168, 48, 48), "tr1_trade"),
            ],
            Kind::Route => vec![
                (Market(0), at(8, 8, 48, 48), "tr_srcobj"),
                (Resource, at(59, 8, 48, 48), "tr_srcres"),
                (Market(1), at(8, 61, 48, 48), "tr_dstobj"),
                (Resource2, at(59, 61, 48, 48), "tr_dstres"),
                (Amount, at(8, 115, 99, 48), "tr_amount"),
                (Cancel, at(8, 168, 48, 48), "tr_cancel"),
                (Accept, at(59, 168, 48, 48), "tr_trade"),
            ],
            Kind::Workshop => vec![
                (Category(0), at(9, 10, 28, 28), "book_itm"),
                (Category(1), at(45, 10, 28, 28), "potion_itm"),
                (Category(2), at(81, 10, 28, 28), "power_itm"),
                (Item, at(8, 61, 48, 48), "bu_item"),
                (Amount, at(59, 61, 48, 48), "bu_amount"),
                (Cancel, at(8, 114, 48, 48), "bu_cancel"),
                (Accept, at(59, 114, 48, 48), "bu_build"),
            ],
        }
    }
    /// Disabled controls (state 3). The workshop's active category also
    /// shows as state 3, its brightest frame, and cannot be pressed.
    pub fn enabled(&self, control: Control, available: [u32; 4]) -> bool {
        use Control::*;
        match self.kind {
            Kind::Gamble => {
                self.rolling == 0
                    && (!matches!(control, Amount | Accept | Odds)
                        || available[self.resource as usize] > 0)
            }
            Kind::Exchange => match control {
                Accept => self.amount <= available[self.resource as usize],
                Received => false,
                _ => true,
            },
            Kind::Route => control != Accept || self.markets.iter().all(Option::is_some),
            Kind::Workshop => match control {
                Category(c) => c != self.option,
                Accept => self.amount <= available[0],
                _ => true,
            },
        }
    }
    /// The number a control shows, if any.
    pub fn number(&self, control: Control) -> Option<u32> {
        match (self.kind, control) {
            (Kind::Exchange, Control::Received) => Some(self.amount / 10),
            (_, Control::Amount) => Some(self.amount),
            _ => None,
        }
    }
    /// 0x4a3b50: the stake fits what the kingdom has, in steps of 50.
    fn clamp_stake(&mut self, available: [u32; 4]) {
        let have = available[self.resource as usize];
        if self.amount > have {
            self.amount = have / 50 * 50;
        }
        if self.amount == 0 {
            self.amount = 50;
        }
    }
    /// A click on a control. `cap` is the trader's load per trip (0x2723);
    /// `minimum` gives the least gold of a book (0x4a5220).
    pub fn click(
        &mut self,
        control: Control,
        right: bool,
        shift: bool,
        available: [u32; 4],
        cap: u32,
        minimum: impl Fn(u8) -> u32,
    ) -> Outcome {
        if !self.enabled(control, available) {
            return Outcome::Nothing;
        }
        let step = |small: i64, big: i64| {
            let s = if shift { big } else { small };
            if right { -s } else { s }
        };
        let add = |v: u32, d: i64| (v as i64 + d).max(0) as u32;
        use Control::*;
        match (self.kind, control) {
            (_, Cancel) => return Outcome::Close,
            (Kind::Gamble, Resource) if !right => {
                self.resource = (self.resource + 1) & 3;
                self.clamp_stake(available);
            }
            (Kind::Gamble, Odds) if !right => self.option = (self.option + 1) & 3,
            (Kind::Gamble, Amount) => {
                self.amount = add(self.amount, step(50, 200));
                self.clamp_stake(available);
            }
            (Kind::Gamble, Accept) if !right => {
                // 0x4a3fee: the roll pays at once; the dice play for about
                // a second with every control disabled.
                self.pending = Some(available[self.resource as usize]);
                self.rolling = DICE_FRAMES * DICE_FRAME_MS;
                let req = Request {
                    resource: self.resource,
                    amount: self.amount,
                    option: self.option,
                    ..Request::command("gamble", vec![self.object])
                };
                return Outcome::Send(req, false);
            }
            (Kind::Exchange | Kind::Route, Resource) if !right => {
                self.resource = (self.resource + 1) & 3
            }
            (Kind::Exchange | Kind::Route, Resource2) if !right => {
                self.resource2 = (self.resource2 + 1) & 3
            }
            (Kind::Exchange, Amount) => {
                self.amount = add(self.amount, step(500, 1000)).clamp(500, 20000);
            }
            (Kind::Exchange, Accept) if !right => {
                let req = Request {
                    resource: self.resource,
                    resource2: self.resource2,
                    amount: self.amount,
                    ..Request::command("trade1", vec![self.object])
                };
                return Outcome::Send(req, true);
            }
            (Kind::Route, Market(k)) if !right => self.picking = Some(k),
            (Kind::Route, Amount) => {
                self.amount = add(self.amount, step(50, 100)).clamp(50, cap.max(50));
            }
            (Kind::Route, Accept) if !right => {
                let req = Request {
                    origin: self.markets[0],
                    target: self.markets[1],
                    resource: self.resource,
                    resource2: self.resource2,
                    amount: self.amount,
                    ..Request::command("trade", vec![self.object])
                };
                return Outcome::Send(req, true);
            }
            (Kind::Workshop, Category(c)) if !right => {
                self.option = c;
                self.item = u8::from(c == 1);
                self.amount = 50;
            }
            (Kind::Workshop, Item) => {
                let n = items(self.option);
                let next = |i: u8| {
                    if right { (i + n - 1) % n } else { (i + 1) % n }
                };
                let mut i = next(self.item);
                // Books skip soulbind (8) and hypnosis (13); potions skip
                // the first.
                if (self.option == 0 && matches!(i, 8 | 13)) || (self.option == 1 && i == 0) {
                    i = next(i);
                }
                self.item = i;
            }
            (Kind::Workshop, Amount) => {
                self.amount = add(self.amount, step(10, 50)).min(available[0]).max(10);
            }
            (Kind::Workshop, Accept) if !right => {
                if self.option == 0 {
                    let least = minimum(self.item);
                    if self.amount < least {
                        // STRINGS3 [msg] goldreq; the dialog stays open.
                        return Outcome::Message(format!(
                            "Questo libro magico richiede almeno {least} unità d'oro"
                        ));
                    }
                }
                let req = Request {
                    option: self.option,
                    item: self.item,
                    amount: self.amount,
                    ..Request::command("build_item", vec![self.object])
                };
                return Outcome::Send(req, true);
            }
            _ => {}
        }
        Outcome::Nothing
    }
    /// Advances the dice and shows the result once the resources change.
    pub fn update(&mut self, available: [u32; 4], elapsed_ms: u32) {
        if let Some(before) = self.pending {
            let now = available[self.resource as usize];
            if now != before {
                self.face = if now > before { 2 } else { 1 };
                self.pending = None;
            }
        }
        if self.rolling > 0 {
            self.rolling = self.rolling.saturating_sub(elapsed_ms);
            if self.rolling == 0 {
                self.pending = None;
                self.clamp_stake(available);
            }
        }
    }
    /// The frame of `zarove.bmp` while the dice roll.
    pub fn dice_frame(&self) -> Option<u32> {
        (self.rolling > 0)
            .then(|| (DICE_FRAMES * DICE_FRAME_MS - self.rolling) / DICE_FRAME_MS % DICE_FRAMES)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const RICH: [u32; 4] = [10000; 4];
    fn press(d: &mut Dialog, c: Control, right: bool, shift: bool) -> Outcome {
        d.click(c, right, shift, RICH, 500, |_| 200)
    }
    #[test]
    fn gambling_steps_and_clamps_the_stake_like_the_original() {
        let mut d = Dialog::gamble(7);
        press(&mut d, Control::Amount, false, true);
        assert_eq!(d.amount, 250);
        press(&mut d, Control::Amount, true, false);
        assert_eq!(d.amount, 200);
        // More than the kingdom has: down to a multiple of 50.
        d.click(Control::Amount, false, false, [120, 0, 0, 0], 0, |_| 0);
        assert_eq!(d.amount, 100);
        // Nothing to stake: the stake, odds and play are disabled.
        assert!(!d.enabled(Control::Accept, [0; 4]));
        for _ in 0..4 {
            press(&mut d, Control::Odds, false, false);
        }
        assert_eq!(d.option, 0);
        let Outcome::Send(req, false) = press(&mut d, Control::Accept, false, false) else {
            panic!("the roll does not close the dialog");
        };
        assert_eq!(
            (req.order.as_str(), req.amount, req.option),
            ("gamble", 100, 0)
        );
        assert!(!d.enabled(Control::Cancel, RICH));
        d.update([10100, 0, 0, 0], 100);
        assert_eq!(d.face, 2);
        d.update(RICH, DICE_FRAMES * DICE_FRAME_MS);
        assert!(d.enabled(Control::Cancel, RICH));
    }
    #[test]
    fn exchange_offers_500_to_20000_for_a_tenth() {
        let mut d = Dialog::exchange(3, None);
        press(&mut d, Control::Amount, true, false);
        assert_eq!(d.amount, 500);
        for _ in 0..30 {
            press(&mut d, Control::Amount, false, true);
        }
        assert_eq!((d.amount, d.number(Control::Received)), (20000, Some(2000)));
        // The same resource on both sides is not refused.
        let Outcome::Send(req, true) = d.click(Control::Accept, false, false, [20000; 4], 0, |_| 0)
        else {
            panic!();
        };
        assert_eq!((req.resource, req.resource2, req.amount), (0, 0, 20000));
        assert!(!d.enabled(Control::Accept, [19999, 0, 0, 0]));
    }
    #[test]
    fn trade_route_needs_both_markets_and_caps_the_load() {
        let mut d = Dialog::route(2, [None; 2], [0, 0], None);
        assert!(!d.enabled(Control::Accept, RICH));
        press(&mut d, Control::Market(1), false, false);
        assert_eq!(d.picking, Some(1));
        d.markets = [Some(10), Some(11)];
        for _ in 0..20 {
            press(&mut d, Control::Amount, false, true);
        }
        assert_eq!(d.amount, 500);
        let Outcome::Send(req, true) = press(&mut d, Control::Accept, false, false) else {
            panic!();
        };
        assert_eq!(
            (req.origin, req.target, req.amount),
            (Some(10), Some(11), 500)
        );
    }
    #[test]
    fn workshop_skips_soulbind_hypnosis_and_the_first_potion() {
        let mut d = Dialog::workshop(5, (0, 7));
        press(&mut d, Control::Item, false, false);
        assert_eq!(d.item, 9);
        d.item = 12;
        press(&mut d, Control::Item, false, false);
        assert_eq!(d.item, 14);
        d.item = 19;
        press(&mut d, Control::Item, false, false);
        assert_eq!(d.item, 0);
        press(&mut d, Control::Category(1), false, false);
        assert_eq!((d.option, d.item, d.amount), (1, 1, 50));
        press(&mut d, Control::Item, true, false);
        assert_eq!(d.item, 4);
        press(&mut d, Control::Item, false, false);
        assert_eq!(d.item, 1);
        // Gold between 10 and what the kingdom has.
        d.click(Control::Amount, false, true, [70, 0, 0, 0], 0, |_| 0);
        assert_eq!(d.amount, 70);
        d.click(Control::Amount, true, true, RICH, 0, |_| 0);
        assert_eq!(d.amount, 20);
        // A book under its minimum keeps the dialog open.
        press(&mut d, Control::Category(0), false, false);
        assert!(matches!(
            press(&mut d, Control::Accept, false, false),
            Outcome::Message(_)
        ));
        d.amount = 200;
        assert!(matches!(
            press(&mut d, Control::Accept, false, false),
            Outcome::Send(_, true)
        ));
    }
}
