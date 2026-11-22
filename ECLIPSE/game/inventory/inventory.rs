pub const HOTBAR_SLOTS: usize = 8;
pub const STORAGE_SLOTS: usize = 24;
pub const INVENTORY_SLOT_COUNT: usize = HOTBAR_SLOTS + STORAGE_SLOTS;
pub const MAX_STACK_SIZE: u16 = 64;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ItemId(pub u16);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ItemStack {
    item: ItemId,
    count: u16,
}

impl ItemStack {
    pub fn new(item: ItemId, count: u16) -> Option<Self> {
        (count > 0 && count <= MAX_STACK_SIZE).then_some(Self { item, count })
    }

    pub fn item(self) -> ItemId {
        self.item
    }

    pub fn count(self) -> u16 {
        self.count
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Inventory {
    slots: [Option<ItemStack>; INVENTORY_SLOT_COUNT],
    cursor: Option<ItemStack>,
    selected_hotbar_slot: usize,
}

impl Default for Inventory {
    fn default() -> Self {
        Self {
            slots: [None; INVENTORY_SLOT_COUNT],
            cursor: None,
            selected_hotbar_slot: 0,
        }
    }
}

impl Inventory {
    pub fn slot(&self, index: usize) -> Option<ItemStack> {
        self.slots.get(index).copied().flatten()
    }

    pub fn cursor_stack(&self) -> Option<ItemStack> {
        self.cursor
    }

    pub fn selected_hotbar_slot(&self) -> usize {
        self.selected_hotbar_slot
    }

    pub fn select_hotbar_slot(&mut self, index: usize) -> bool {
        if index >= HOTBAR_SLOTS {
            return false;
        }
        self.selected_hotbar_slot = index;
        true
    }

    pub fn insert(&mut self, item: ItemId, count: u16) -> u16 {
        let mut remaining = count;
        for stack in self.slots.iter_mut().flatten() {
            if stack.item == item && stack.count < MAX_STACK_SIZE {
                let moved = remaining.min(MAX_STACK_SIZE - stack.count);
                stack.count += moved;
                remaining -= moved;
                if remaining == 0 {
                    return 0;
                }
            }
        }

        for slot in &mut self.slots {
            if slot.is_none() {
                let moved = remaining.min(MAX_STACK_SIZE);
                *slot = ItemStack::new(item, moved);
                remaining -= moved;
                if remaining == 0 {
                    return 0;
                }
            }
        }
        remaining
    }

    pub fn take(&mut self, index: usize, count: u16) -> Option<ItemStack> {
        if count == 0 {
            return None;
        }
        let slot = self.slots.get_mut(index)?;
        let stack = slot.as_mut()?;
        let taken = count.min(stack.count);
        let result = ItemStack {
            item: stack.item,
            count: taken,
        };
        stack.count -= taken;
        if stack.count == 0 {
            *slot = None;
        }
        Some(result)
    }

    pub fn click_slot(&mut self, index: usize) -> bool {
        let Some(slot) = self.slots.get(index) else {
            return false;
        };
        let target = *slot;
        match (self.cursor, target) {
            (None, None) => false,
            (None, Some(_)) => {
                self.cursor = self.slots[index].take();
                true
            }
            (Some(stack), None) => {
                self.cursor = None;
                self.slots[index] = Some(stack);
                true
            }
            (Some(mut held), Some(mut target)) if held.item == target.item => {
                let moved = held.count.min(MAX_STACK_SIZE - target.count);
                if moved == 0 {
                    return false;
                }
                target.count += moved;
                held.count -= moved;
                self.slots[index] = Some(target);
                self.cursor = ItemStack::new(held.item, held.count);
                true
            }
            (Some(held), Some(target)) => {
                self.slots[index] = Some(held);
                self.cursor = Some(target);
                true
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inserting_stacks_and_taking_items_preserves_counts() {
        let mut inventory = Inventory::default();
        let item = ItemId(7);
        assert_eq!(inventory.insert(item, 70), 0);
        assert_eq!(inventory.slot(0).unwrap().count(), 64);
        assert_eq!(inventory.slot(1).unwrap().count(), 6);

        assert_eq!(inventory.take(0, 10).unwrap().count(), 10);
        assert_eq!(inventory.slot(0).unwrap().count(), 54);
    }

    #[test]
    fn inventory_returns_overflow_and_rejects_invalid_hotbar_slots() {
        let mut inventory = Inventory::default();
        assert_eq!(
            inventory.insert(
                ItemId(1),
                (INVENTORY_SLOT_COUNT as u16) * MAX_STACK_SIZE + 1
            ),
            1
        );
        assert!(!inventory.select_hotbar_slot(HOTBAR_SLOTS));
        assert!(inventory.select_hotbar_slot(HOTBAR_SLOTS - 1));
        assert_eq!(inventory.selected_hotbar_slot(), HOTBAR_SLOTS - 1);
    }

    #[test]
    fn clicking_slots_picks_up_places_and_swaps_stacks() {
        let mut inventory = Inventory::default();
        inventory.insert(ItemId(1), 3);
        inventory.insert(ItemId(2), 5);

        assert!(inventory.click_slot(0));
        assert_eq!(inventory.cursor_stack().unwrap().item(), ItemId(1));
        assert!(inventory.click_slot(1));
        assert_eq!(inventory.slot(1).unwrap().item(), ItemId(1));
        assert_eq!(inventory.cursor_stack().unwrap().item(), ItemId(2));
        assert!(inventory.click_slot(2));
        assert_eq!(inventory.slot(2).unwrap().item(), ItemId(2));
        assert_eq!(inventory.cursor_stack(), None);
    }
}
