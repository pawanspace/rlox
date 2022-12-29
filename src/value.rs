use crate::common;
#[derive(Debug, Clone)]
pub(crate) struct ValueArray {
    pub values: Vec<common::Value>,
}

impl ValueArray {
    pub(crate) fn init() -> ValueArray {
        ValueArray { values: vec![] }
    }

    pub(crate) fn append(&mut self, value: common::Value) {
        self.values.push(value);
    }

    pub(crate) fn get(&self, index: usize) -> common::Value {
        // TODO: I think unwrapping here is kind of unsafe so I should
        // use better approach to handle all the unwraps
        (*self.values.get(index).unwrap()).clone()
    }

    pub(crate) fn count(&self) -> usize {
        self.values.len() - 1
    }
}
