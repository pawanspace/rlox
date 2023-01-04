pub(crate) struct Compiler<'c> {
    table: &'c mut Table<Value>,
    scanner: Scanner,
    parser: Parser,
    source: String,
    current_context: usize,
    scope_depth: usize,
    contexts: Vec<CompilerContext>
}