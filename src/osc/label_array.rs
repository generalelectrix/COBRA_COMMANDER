use rosc::OscType;

use super::ScopedOscMessage;

/// Model a 1D array of labels.
/// Allows dynamic remapping of a subset of labels.
/// Each individual label should accept an address of the form /group/control/n
#[derive(Clone)]
pub struct LabelArray {
    pub control: &'static str,
    pub empty_label: &'static str,
    pub n: usize,
}

impl LabelArray {
    /// Write labels to this array.
    /// If there are more labels provided than defined for this array,
    /// the extra lables are ignored.
    pub fn set<S>(&self, labels: impl Iterator<Item = String>, emitter: &S)
    where
        S: crate::osc::EmitScopedOscMessage + ?Sized,
    {
        for (i, label) in labels
            .chain(std::iter::repeat(self.empty_label).map(String::from))
            .enumerate()
        {
            if i >= self.n {
                return;
            }
            emitter.emit_osc(ScopedOscMessage {
                control: &format!("{}/{i}", self.control),
                arg: OscType::String(label),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::osc::MockEmitter;

    fn make_label_array() -> LabelArray {
        LabelArray {
            control: "Lbl",
            empty_label: "-",
            n: 4,
        }
    }

    #[test]
    fn test_set_fills_labels() {
        let la = make_label_array();
        let emitter = MockEmitter::new();
        la.set(vec!["A".to_string(), "B".to_string()].into_iter(), &emitter);
        let msgs = emitter.take();
        assert_eq!(msgs.len(), 4);
        assert_eq!(msgs[0], ("Lbl/0".to_string(), OscType::String("A".to_string())));
        assert_eq!(msgs[1], ("Lbl/1".to_string(), OscType::String("B".to_string())));
        assert_eq!(msgs[2], ("Lbl/2".to_string(), OscType::String("-".to_string())));
        assert_eq!(msgs[3], ("Lbl/3".to_string(), OscType::String("-".to_string())));
    }

    #[test]
    fn test_set_truncates_excess() {
        let la = LabelArray {
            control: "Lbl",
            empty_label: "-",
            n: 3,
        };
        let emitter = MockEmitter::new();
        let labels = vec!["A", "B", "C", "D", "E"]
            .into_iter()
            .map(String::from);
        la.set(labels, &emitter);
        let msgs = emitter.take();
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[0], ("Lbl/0".to_string(), OscType::String("A".to_string())));
        assert_eq!(msgs[1], ("Lbl/1".to_string(), OscType::String("B".to_string())));
        assert_eq!(msgs[2], ("Lbl/2".to_string(), OscType::String("C".to_string())));
    }
}
