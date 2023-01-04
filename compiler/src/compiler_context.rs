#[derive(Debug, Clone, Copy)]
pub(crate) enum Local {
    Filled(Token, usize),
    Empty,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum UpValue {
    Filled(u8, bool),
    Empty,
}

#[derive(Debug, Clone)]
pub(crate) struct CompilerContext {
    function: Obj,
    locals: Vec<Local>,
    local_count: usize,
    up_values: Vec<UpValue>,
    up_value_count: usize,
}