use crate::common;

#[derive(Debug)]
pub(crate) struct ValueArray<'a> {
    pub values: Vec<&'a common::Value>,
}

impl<'a> ValueArray<'a> {
    pub(crate) fn init() -> ValueArray<'a> {
        ValueArray { values: vec![] }
    }

    pub(crate) fn append(&mut self, value: &'a common::Value) {
        self.values.push(value);
    }

    pub(crate) fn get(&self, index: usize) -> &'a common::Value {
        // TODO: I think unwrapping here is kind of unsafe so I should
        // use better approach to handle all the unwraps
        self.values.get(index).unwrap()
    }

    pub(crate) fn count(&self) -> usize {
        self.values.len() - 1
    }
}
