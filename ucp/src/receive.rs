use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::packets::Frame;

type Fragmented = (u32, BTreeMap<u32, Vec<u8>>); //size,bytes

pub(crate) struct ReceiveQueue {
    ack: BTreeSet<u32>,
    ack_next: u32,
    ack_missing: BTreeSet<u32>,
    ordered: BTreeMap<u32, Vec<u8>>,
    ordered_next: u32,

    fragment: HashMap<u16, Fragmented>,
}

impl ReceiveQueue {
    pub fn new() -> Self {
        Self {
            ack: BTreeSet::new(),
            ack_next: 0,
            ack_missing: BTreeSet::new(),
            ordered: BTreeMap::new(),
            ordered_next: 0,
            fragment: HashMap::new(),
        }
    }

    pub fn received(&mut self, seq: u32) {
        if seq >= self.ack_next {
            self.ack.insert(seq);
        }
    }

    pub fn get_ack(&mut self) -> Option<(u32, u32)> {
        let first = *self.ack.iter().next()?;
        while first != self.ack_next {
            self.ack_missing.insert(self.ack_next);
            self.ack_next += 1;
        }
        let last = *self.ack.iter().next_back()? + 1;
        let mut ret = (first, first);

        for i in first..last {
            if self.ack.remove(&i) {
                ret.1 = i;
                self.ack_next = i + 1;
                continue;
            }
            break;
        }
        Some(ret)
    }

    pub fn get_nack(&mut self) -> Option<(u32, u32)> {
        let first = *self.ack_missing.iter().next()?;
        let last = *self.ack_missing.iter().next_back()? + 1;
        let mut ret = (first, first);
        for i in first..last {
            if self.ack_missing.remove(&i) {
                ret.1 = i;
                continue;
            }
            break;
        }
        Some(ret)
    }

    pub fn fragmented(&mut self, frame: Frame, bytes: &[u8]) -> Option<Vec<u8>> {
        if let Some(fragment) = frame.fragment {
            if let std::collections::hash_map::Entry::Vacant(e) = self.fragment.entry(fragment.id) {
                let mut bmap = BTreeMap::new();
                bmap.insert(fragment.index, bytes.to_vec());
                e.insert((fragment.size, bmap));
            } else {
                let mng = self.fragment.get_mut(&fragment.id).unwrap();
                mng.1.insert(fragment.index, bytes.to_vec());
                if mng.0 as usize == mng.1.len() {
                    let mut ret = vec![];
                    for i in 0..mng.0 {
                        ret.append(&mut mng.1.remove(&i).unwrap());
                    }
                    if !frame.reliability.sequenced() && !frame.reliability.ordered() {
                        return Some(ret);
                    } else {
                        self._ordered(ret, frame.oindex);
                        return None;
                    }
                }
            }
        }
        None
    }

    fn _ordered(&mut self, data: Vec<u8>, order_index: u32) {
        if order_index >= self.ordered_next {
            self.ordered.insert(order_index, data);
        }
    }

    pub fn ordered(&mut self, frame: Frame, bytes: &[u8]) {
        if frame.oindex >= self.ordered_next {
            self.ordered.insert(frame.oindex, bytes.to_vec());
        }
    }

    pub fn next_ordered(&mut self) -> Option<Vec<u8>> {
        let first = *self.ordered.iter().next()?.0;
        if first == self.ordered_next {
            self.ordered_next = first + 1;
            return self.ordered.remove(&first);
        }
        return None;
    }
}

/*#[cfg(test)]
mod test {
    use crate::packets::{Frame, Reliability};

    use super::ReceiveQueue;
    #[test]
    fn ack_queue() {
        let mut receive = ReceiveQueue::new();
        for i in 0..4 {
            receive.received(i);
        }
        // 4 and 5 are missing
        for i in 6..10 {
            receive.received(i);
        }

        assert_eq!(receive.get_ack(), Some((0, 3)));
        assert_eq!(receive.get_ack(), Some((6, 9)));
        assert_eq!(receive.get_ack(), None);
        assert_eq!(receive.get_nack(), Some((4, 5)));
        assert_eq!(receive.get_nack(), None);
    }

    #[test]
    fn order_queue() {
        let mut receive = ReceiveQueue::new();
        let data = b"";
        for i in 0..2 {
            receive.ordered(Frame {
                reliability: Reliability::ReliableOrdered,
                length : 0,
                fragment: None,
                mindex: 0,
                sindex: 0,
                oindex: i,
            });
        }
        // 2 and 3 are missing
        for i in 4..5 {
            receive.ordered(Frame {
                reliability: Reliability::ReliableOrdered,
                length : 0,
                fragment: None,
                mindex: 0,
                sindex: 0,
                oindex: i,
            });
        }

        assert_eq!(receive.next_ordered(), Some(vec![])); //order index = 0
        assert_eq!(receive.next_ordered(), Some(vec![])); //order index = 1
        assert_eq!(receive.next_ordered(), None);
        //add 2 and 3
        receive.ordered(Frame {
            reliability: Reliability::ReliableOrdered,
            length : 0,
            fragment: None,
            mindex: 0,
            sindex: 0,
            oindex: 2,
        });
        receive.ordered(Frame {
            reliability: Reliability::ReliableOrdered,
            length : 0,
            fragment: None,
            mindex: 0,
            sindex: 0,
            oindex: 3,
        });
        assert_eq!(receive.next_ordered(), Some(vec![])); //order index = 2
        assert_eq!(receive.next_ordered(), Some(vec![])); //order index = 3
        assert_eq!(receive.next_ordered(), Some(vec![])); //order index = 4
        assert_eq!(receive.next_ordered(), None);
    }
}
*/
