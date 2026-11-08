//! Undo/redo. An edit is a [`Command`] applied to a document's data; the same commands will be
//! issued by the MCP server, so agent edits are undoable like the user's.
pub trait Command<T> {
    fn apply(&mut self, target: &mut T);
    fn revert(&mut self, target: &mut T);
    fn label(&self) -> String;
}

pub struct History<T> {
    done: Vec<Box<dyn Command<T>>>,
    undone: Vec<Box<dyn Command<T>>>,
    /// length of `done` at the last save
    saved_at: Option<usize>,
}

impl<T> Default for History<T> {
    fn default() -> Self {
        History { done: Vec::new(), undone: Vec::new(), saved_at: Some(0) }
    }
}

impl<T> History<T> {
    pub fn run(&mut self, mut c: Box<dyn Command<T>>, target: &mut T) {
        c.apply(target);
        self.done.push(c);
        self.undone.clear();
        if self.saved_at.is_some_and(|s| s >= self.done.len()) {
            self.saved_at = None;
        }
    }
    pub fn undo(&mut self, target: &mut T) {
        if let Some(mut c) = self.done.pop() {
            c.revert(target);
            self.undone.push(c);
        }
    }
    pub fn redo(&mut self, target: &mut T) {
        if let Some(mut c) = self.undone.pop() {
            c.apply(target);
            self.done.push(c);
        }
    }
    pub fn can_undo(&self) -> bool {
        !self.done.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.undone.is_empty()
    }
    pub fn mark_saved(&mut self) {
        self.saved_at = Some(self.done.len());
    }
    pub fn dirty(&self) -> bool {
        self.saved_at != Some(self.done.len())
    }
    pub fn labels(&self) -> impl Iterator<Item = String> + '_ {
        self.done.iter().map(|c| c.label())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Add(i32);
    impl Command<i32> for Add {
        fn apply(&mut self, t: &mut i32) {
            *t += self.0;
        }
        fn revert(&mut self, t: &mut i32) {
            *t -= self.0;
        }
        fn label(&self) -> String {
            format!("add {}", self.0)
        }
    }

    #[test]
    fn undo_redo_dirty() {
        let mut h = History::default();
        let mut v = 0;
        assert!(!h.dirty());
        h.run(Box::new(Add(2)), &mut v);
        h.run(Box::new(Add(3)), &mut v);
        assert_eq!(v, 5);
        assert!(h.dirty());
        h.mark_saved();
        h.undo(&mut v);
        assert_eq!(v, 2);
        assert!(h.dirty());
        h.redo(&mut v);
        assert_eq!(v, 5);
        assert!(!h.dirty());
        h.undo(&mut v);
        h.run(Box::new(Add(10)), &mut v);
        assert!(!h.can_redo());
        assert_eq!(v, 12);
        assert!(h.dirty());
    }
}
