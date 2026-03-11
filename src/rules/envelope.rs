use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use oxc_allocator::Allocator;
use oxc_ast::ast::{
    Argument, ArrayExpressionElement, BindingPatternKind, CallExpression, ChainElement,
    Declaration, ExportDefaultDeclarationKind, Expression, ForStatementInit, ForStatementLeft,
    Function, ObjectPropertyKind, Statement,
};
use oxc_parser::Parser;
use oxc_span::SourceType;

use crate::parser::is_import_type_only;

/// Result of analyzing a single file for envelope compliance.
#[derive(Debug, Clone)]
pub struct EnvelopeAnalysis {
    pub has_envelope_import: bool,
    pub import_bindings: Vec<String>,
    pub is_type_only_import: bool,
    pub calls: Vec<EnvelopeCall>,
}

#[derive(Debug, Clone)]
pub struct EnvelopeCall {
    pub contract_id: String,
    pub line: usize,
    pub column: usize,
    pub span_start: u32,
    pub span_end: u32,
}

#[derive(Debug)]
pub enum EnvelopeError {
    FileRead(std::io::Error),
    Parse(String),
}

pub fn analyze_file_for_envelope(
    path: &Path,
    import_patterns: &[String],
    function_pattern: &str,
    match_pattern: Option<&str>,
) -> Result<EnvelopeAnalysis, EnvelopeError> {
    let source = fs::read_to_string(path).map_err(EnvelopeError::FileRead)?;

    let allocator = Allocator::default();
    let source_type =
        SourceType::from_path(path).unwrap_or_else(|_| SourceType::default().with_module(true));
    let parser_return = Parser::new(&allocator, &source, source_type).parse();

    if !parser_return.errors.is_empty() {
        let message = parser_return
            .errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("; ");
        return Err(EnvelopeError::Parse(message));
    }

    let mapper = SourceMapper::new(&source);
    let mut state = AnalyzerState::new(import_patterns, function_pattern);

    collect_import_info(&parser_return.program.body, &mut state);

    for statement in &parser_return.program.body {
        visit_statement_for_calls(statement, &mapper, &mut state);
    }

    if let Some(function_name) = match_pattern {
        if let Some((span_start, span_end)) =
            find_exported_function_span_in_statements(&parser_return.program.body, function_name)
        {
            state
                .calls
                .retain(|call| call.span_start >= span_start && call.span_end <= span_end);
        } else {
            state.calls.clear();
        }
    }

    let mut import_bindings = state.import_bindings.into_iter().collect::<Vec<_>>();
    import_bindings.sort();

    state.calls.sort_by(|a, b| {
        a.line
            .cmp(&b.line)
            .then_with(|| a.column.cmp(&b.column))
            .then_with(|| a.contract_id.cmp(&b.contract_id))
    });

    Ok(EnvelopeAnalysis {
        has_envelope_import: state.has_envelope_import,
        import_bindings,
        is_type_only_import: state.is_type_only_import,
        calls: state.calls,
    })
}

/// Find the byte span (start, end) of an exported function/const matching the given name.
/// Returns None if no matching export is found.
///
/// Handles:
/// - `export function createUser(...) { ... }`
/// - `export async function createUser(...) { ... }`
/// - `export const createUser = (...) => { ... }`
/// - `export const createUser = async (...) => { ... }`
/// - `export const createUser = function(...) { ... }`
/// - `export default function createUser(...) { ... }`
pub fn find_exported_function_span(source: &str, function_name: &str) -> Option<(u32, u32)> {
    let allocator = Allocator::default();
    let source_type = SourceType::default().with_module(true);
    let parser_return = Parser::new(&allocator, source, source_type).parse();

    if !parser_return.errors.is_empty() {
        return None;
    }

    find_exported_function_span_in_statements(&parser_return.program.body, function_name)
}

fn find_exported_function_span_in_statements(
    statements: &[Statement<'_>],
    function_name: &str,
) -> Option<(u32, u32)> {
    for statement in statements {
        if let Some(span) = exported_function_span_from_statement(statement, function_name) {
            return Some(span);
        }
    }

    for statement in statements {
        if let Statement::ExportNamedDeclaration(decl) = statement
            && decl.declaration.is_none()
            && decl.source.is_none()
            && decl
                .specifiers
                .iter()
                .any(|specifier| specifier.local.name() == function_name)
        {
            return find_local_function_span(statements, function_name);
        }
    }

    None
}

fn exported_function_span_from_statement<'a>(
    statement: &Statement<'a>,
    function_name: &str,
) -> Option<(u32, u32)> {
    match statement {
        Statement::ExportNamedDeclaration(decl) => decl
            .declaration
            .as_ref()
            .and_then(|declaration| declaration_span(declaration, function_name)),
        Statement::ExportDefaultDeclaration(decl) => {
            if let ExportDefaultDeclarationKind::FunctionDeclaration(function) = &decl.declaration {
                return named_function_body_span(function, function_name);
            }

            None
        }
        _ => None,
    }
}

fn declaration_span(declaration: &Declaration<'_>, function_name: &str) -> Option<(u32, u32)> {
    match declaration {
        Declaration::FunctionDeclaration(function) => {
            named_function_body_span(function, function_name)
        }
        Declaration::VariableDeclaration(declaration) => {
            variable_declaration_span(declaration, function_name)
        }
        Declaration::ClassDeclaration(class_declaration) => {
            class_declaration_method_span(class_declaration, function_name)
        }
        _ => None,
    }
}

fn named_function_body_span(function: &Function<'_>, function_name: &str) -> Option<(u32, u32)> {
    if function
        .id
        .as_ref()
        .is_some_and(|identifier| identifier.name == function_name)
    {
        return function_expression_body_span(function.body.as_ref());
    }

    None
}

fn variable_declaration_span(
    declaration: &oxc_ast::ast::VariableDeclaration<'_>,
    function_name: &str,
) -> Option<(u32, u32)> {
    for declarator in &declaration.declarations {
        if declaration_name_matches(&declarator.id, function_name)
            && let Some(init) = &declarator.init
            && let Some(span) = function_like_initializer_span(init)
        {
            return Some(span);
        }
    }

    None
}

fn function_like_initializer_span(expression: &Expression<'_>) -> Option<(u32, u32)> {
    match expression {
        Expression::ArrowFunctionExpression(function) => {
            Some((function.body.span.start, function.body.span.end))
        }
        Expression::FunctionExpression(function) => {
            function_expression_body_span(function.body.as_ref())
        }
        Expression::CallExpression(call_expression)
            if call_expression
                .arguments
                .iter()
                .filter_map(Argument::as_expression)
                .any(|argument| {
                    matches!(
                        argument,
                        Expression::ArrowFunctionExpression(_) | Expression::FunctionExpression(_)
                    )
                }) =>
        {
            Some((call_expression.span.start, call_expression.span.end))
        }
        _ => None,
    }
}

fn class_declaration_method_span(
    class_declaration: &oxc_ast::ast::Class<'_>,
    function_name: &str,
) -> Option<(u32, u32)> {
    for element in &class_declaration.body.body {
        if let oxc_ast::ast::ClassElement::MethodDefinition(method) = element
            && method
                .key
                .static_name()
                .is_some_and(|method_name| method_name == function_name)
        {
            return function_expression_body_span(method.value.body.as_ref());
        }
    }

    None
}

fn find_local_function_span(
    statements: &[Statement<'_>],
    function_name: &str,
) -> Option<(u32, u32)> {
    for statement in statements {
        match statement {
            Statement::FunctionDeclaration(function) => {
                if let Some(span) = named_function_body_span(function, function_name) {
                    return Some(span);
                }
            }
            Statement::VariableDeclaration(declaration) => {
                if let Some(span) = variable_declaration_span(declaration, function_name) {
                    return Some(span);
                }
            }
            Statement::ClassDeclaration(class_declaration) => {
                if let Some(span) = class_declaration_method_span(class_declaration, function_name)
                {
                    return Some(span);
                }
            }
            _ => {}
        }
    }

    None
}

fn function_expression_body_span(
    body: Option<&oxc_allocator::Box<'_, oxc_ast::ast::FunctionBody<'_>>>,
) -> Option<(u32, u32)> {
    body.map(|body| (body.span.start, body.span.end))
}

fn declaration_name_matches(declarator: &oxc_ast::ast::BindingPattern<'_>, name: &str) -> bool {
    match &declarator.kind {
        BindingPatternKind::BindingIdentifier(identifier) => identifier.name == name,
        BindingPatternKind::AssignmentPattern(assignment) => {
            declaration_name_matches(&assignment.left, name)
        }
        _ => false,
    }
}

/// Filter envelope calls to only those within a given byte span range.
pub fn filter_calls_by_span(
    calls: &[EnvelopeCall],
    span_start: u32,
    span_end: u32,
) -> Vec<&EnvelopeCall> {
    calls
        .iter()
        .filter(|call| call.span_start >= span_start && call.span_end <= span_end)
        .collect()
}

struct AnalyzerState<'a> {
    import_patterns: &'a [String],
    pattern_parts: Vec<String>,
    import_bindings: BTreeSet<String>,
    namespace_import_bindings: BTreeSet<String>,
    has_envelope_import: bool,
    is_type_only_import: bool,
    calls: Vec<EnvelopeCall>,
}

impl<'a> AnalyzerState<'a> {
    fn new(import_patterns: &'a [String], function_pattern: &str) -> Self {
        let pattern_parts = function_pattern
            .split('.')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .map(ToString::to_string)
            .collect::<Vec<_>>();

        Self {
            import_patterns,
            pattern_parts,
            import_bindings: BTreeSet::new(),
            namespace_import_bindings: BTreeSet::new(),
            has_envelope_import: false,
            is_type_only_import: false,
            calls: Vec::new(),
        }
    }

    fn matches_import_pattern(&self, value: &str) -> bool {
        self.import_patterns.iter().any(|pattern| pattern == value)
    }

    fn is_import_binding(&self, value: &str) -> bool {
        self.import_bindings.contains(value)
    }

    fn is_namespace_import_binding(&self, value: &str) -> bool {
        self.namespace_import_bindings.contains(value)
    }
}

fn collect_import_info(statements: &[Statement<'_>], state: &mut AnalyzerState<'_>) {
    let mut saw_type_only_envelope_import = false;
    let mut saw_runtime_envelope_import = false;

    for statement in statements {
        if let Statement::ImportDeclaration(decl) = statement {
            let specifier = decl.source.value.as_str();
            if !state.matches_import_pattern(specifier) {
                continue;
            }

            if is_import_type_only(decl) {
                saw_type_only_envelope_import = true;
                continue;
            }

            saw_runtime_envelope_import = true;

            let mut has_runtime_binding = false;
            if let Some(specifiers) = &decl.specifiers {
                for specifier in specifiers {
                    match specifier {
                        oxc_ast::ast::ImportDeclarationSpecifier::ImportSpecifier(specifier) => {
                            state
                                .import_bindings
                                .insert(specifier.local.name.to_string());
                            has_runtime_binding = true;
                        }
                        oxc_ast::ast::ImportDeclarationSpecifier::ImportDefaultSpecifier(
                            specifier,
                        ) => {
                            state
                                .import_bindings
                                .insert(specifier.local.name.to_string());
                            has_runtime_binding = true;
                        }
                        oxc_ast::ast::ImportDeclarationSpecifier::ImportNamespaceSpecifier(
                            specifier,
                        ) => {
                            state
                                .namespace_import_bindings
                                .insert(specifier.local.name.to_string());
                            has_runtime_binding = true;
                        }
                    }
                }
            }

            if has_runtime_binding {
                state.has_envelope_import = true;
            }
        }
    }

    state.is_type_only_import = saw_type_only_envelope_import && !saw_runtime_envelope_import;
}

fn visit_statement_for_calls(
    statement: &Statement<'_>,
    mapper: &SourceMapper,
    state: &mut AnalyzerState<'_>,
) {
    match statement {
        Statement::BlockStatement(block) => {
            for nested in &block.body {
                visit_statement_for_calls(nested, mapper, state);
            }
        }
        Statement::ExpressionStatement(expr_stmt) => {
            visit_expression_for_calls(&expr_stmt.expression, mapper, state);
        }
        Statement::DoWhileStatement(stmt) => {
            visit_statement_for_calls(&stmt.body, mapper, state);
            visit_expression_for_calls(&stmt.test, mapper, state);
        }
        Statement::WhileStatement(stmt) => {
            visit_expression_for_calls(&stmt.test, mapper, state);
            visit_statement_for_calls(&stmt.body, mapper, state);
        }
        Statement::ForStatement(stmt) => {
            if let Some(init) = &stmt.init {
                visit_for_init(init, mapper, state);
            }
            if let Some(test) = &stmt.test {
                visit_expression_for_calls(test, mapper, state);
            }
            if let Some(update) = &stmt.update {
                visit_expression_for_calls(update, mapper, state);
            }
            visit_statement_for_calls(&stmt.body, mapper, state);
        }
        Statement::ForInStatement(stmt) => {
            visit_for_left(&stmt.left, mapper, state);
            visit_expression_for_calls(&stmt.right, mapper, state);
            visit_statement_for_calls(&stmt.body, mapper, state);
        }
        Statement::ForOfStatement(stmt) => {
            visit_for_left(&stmt.left, mapper, state);
            visit_expression_for_calls(&stmt.right, mapper, state);
            visit_statement_for_calls(&stmt.body, mapper, state);
        }
        Statement::IfStatement(stmt) => {
            visit_expression_for_calls(&stmt.test, mapper, state);
            visit_statement_for_calls(&stmt.consequent, mapper, state);
            if let Some(alternate) = &stmt.alternate {
                visit_statement_for_calls(alternate, mapper, state);
            }
        }
        Statement::ReturnStatement(stmt) => {
            if let Some(argument) = &stmt.argument {
                visit_expression_for_calls(argument, mapper, state);
            }
        }
        Statement::SwitchStatement(stmt) => {
            visit_expression_for_calls(&stmt.discriminant, mapper, state);
            for case in &stmt.cases {
                if let Some(test) = &case.test {
                    visit_expression_for_calls(test, mapper, state);
                }
                for nested in &case.consequent {
                    visit_statement_for_calls(nested, mapper, state);
                }
            }
        }
        Statement::ThrowStatement(stmt) => {
            visit_expression_for_calls(&stmt.argument, mapper, state);
        }
        Statement::TryStatement(stmt) => {
            for nested in &stmt.block.body {
                visit_statement_for_calls(nested, mapper, state);
            }
            if let Some(handler) = &stmt.handler {
                for nested in &handler.body.body {
                    visit_statement_for_calls(nested, mapper, state);
                }
            }
            if let Some(finalizer) = &stmt.finalizer {
                for nested in &finalizer.body {
                    visit_statement_for_calls(nested, mapper, state);
                }
            }
        }
        Statement::WithStatement(stmt) => {
            visit_expression_for_calls(&stmt.object, mapper, state);
            visit_statement_for_calls(&stmt.body, mapper, state);
        }
        Statement::LabeledStatement(stmt) => {
            visit_statement_for_calls(&stmt.body, mapper, state);
        }
        Statement::VariableDeclaration(declaration) => {
            for declarator in &declaration.declarations {
                if let Some(init) = &declarator.init {
                    visit_expression_for_calls(init, mapper, state);
                }
            }
        }
        Statement::FunctionDeclaration(function) => {
            visit_function(function, mapper, state);
        }
        Statement::ClassDeclaration(class_decl) => {
            visit_class(class_decl, mapper, state);
        }
        Statement::ExportNamedDeclaration(decl) => {
            if let Some(inner_declaration) = &decl.declaration {
                visit_declaration(inner_declaration, mapper, state);
            }
        }
        Statement::ExportDefaultDeclaration(decl) => {
            if let Some(expr) = decl.declaration.as_expression() {
                visit_expression_for_calls(expr, mapper, state);
            }
        }
        _ => {}
    }
}

fn visit_declaration(
    declaration: &Declaration<'_>,
    mapper: &SourceMapper,
    state: &mut AnalyzerState<'_>,
) {
    match declaration {
        Declaration::VariableDeclaration(declaration) => {
            for declarator in &declaration.declarations {
                if let Some(init) = &declarator.init {
                    visit_expression_for_calls(init, mapper, state);
                }
            }
        }
        Declaration::FunctionDeclaration(function) => {
            visit_function(function, mapper, state);
        }
        Declaration::ClassDeclaration(class_decl) => {
            visit_class(class_decl, mapper, state);
        }
        _ => {}
    }
}

fn visit_function(function: &Function<'_>, mapper: &SourceMapper, state: &mut AnalyzerState<'_>) {
    if let Some(body) = &function.body {
        for statement in &body.statements {
            visit_statement_for_calls(statement, mapper, state);
        }
    }
}

fn visit_class(
    class_decl: &oxc_ast::ast::Class<'_>,
    mapper: &SourceMapper,
    state: &mut AnalyzerState<'_>,
) {
    if let Some(super_class) = &class_decl.super_class {
        visit_expression_for_calls(super_class, mapper, state);
    }

    for element in &class_decl.body.body {
        match element {
            oxc_ast::ast::ClassElement::StaticBlock(block) => {
                for statement in &block.body {
                    visit_statement_for_calls(statement, mapper, state);
                }
            }
            oxc_ast::ast::ClassElement::MethodDefinition(method) => {
                visit_function(&method.value, mapper, state);
            }
            oxc_ast::ast::ClassElement::PropertyDefinition(property) => {
                if let Some(value) = &property.value {
                    visit_expression_for_calls(value, mapper, state);
                }
            }
            oxc_ast::ast::ClassElement::AccessorProperty(property) => {
                if let Some(value) = &property.value {
                    visit_expression_for_calls(value, mapper, state);
                }
            }
            _ => {}
        }
    }
}

fn visit_for_init(
    init: &ForStatementInit<'_>,
    mapper: &SourceMapper,
    state: &mut AnalyzerState<'_>,
) {
    match init {
        ForStatementInit::VariableDeclaration(declaration) => {
            for declarator in &declaration.declarations {
                if let Some(expr) = &declarator.init {
                    visit_expression_for_calls(expr, mapper, state);
                }
            }
        }
        _ => {
            if let Some(expr) = init.as_expression() {
                visit_expression_for_calls(expr, mapper, state);
            }
        }
    }
}

fn visit_for_left(
    left: &ForStatementLeft<'_>,
    mapper: &SourceMapper,
    state: &mut AnalyzerState<'_>,
) {
    match left {
        ForStatementLeft::VariableDeclaration(declaration) => {
            for declarator in &declaration.declarations {
                if let Some(expr) = &declarator.init {
                    visit_expression_for_calls(expr, mapper, state);
                }
            }
        }
        _ => {
            if let Some(target) = left.as_assignment_target()
                && let Some(expr) = target.get_expression()
            {
                visit_expression_for_calls(expr, mapper, state);
            }
        }
    }
}

fn visit_expression_for_calls(
    expression: &Expression<'_>,
    mapper: &SourceMapper,
    state: &mut AnalyzerState<'_>,
) {
    match expression {
        Expression::CallExpression(call) => {
            collect_call_expression(call, mapper, state);

            visit_expression_for_calls(&call.callee, mapper, state);
            for argument in &call.arguments {
                visit_argument(argument, mapper, state);
            }
        }
        Expression::ArrayExpression(array_expression) => {
            for element in &array_expression.elements {
                visit_array_element(element, mapper, state);
            }
        }
        Expression::ObjectExpression(object_expression) => {
            for property in &object_expression.properties {
                match property {
                    ObjectPropertyKind::ObjectProperty(property) => {
                        if property.computed
                            && let Some(key) = property.key.as_expression()
                        {
                            visit_expression_for_calls(key, mapper, state);
                        }
                        visit_expression_for_calls(&property.value, mapper, state);
                    }
                    ObjectPropertyKind::SpreadProperty(spread) => {
                        visit_expression_for_calls(&spread.argument, mapper, state);
                    }
                }
            }
        }
        Expression::TemplateLiteral(template_literal) => {
            for expr in &template_literal.expressions {
                visit_expression_for_calls(expr, mapper, state);
            }
        }
        Expression::TaggedTemplateExpression(tagged) => {
            visit_expression_for_calls(&tagged.tag, mapper, state);
            for expr in &tagged.quasi.expressions {
                visit_expression_for_calls(expr, mapper, state);
            }
        }
        Expression::UnaryExpression(unary) => {
            visit_expression_for_calls(&unary.argument, mapper, state);
        }
        Expression::UpdateExpression(update) => {
            if let Some(expr) = update.argument.get_expression() {
                visit_expression_for_calls(expr, mapper, state);
            }
        }
        Expression::BinaryExpression(binary) => {
            visit_expression_for_calls(&binary.left, mapper, state);
            visit_expression_for_calls(&binary.right, mapper, state);
        }
        Expression::LogicalExpression(logical) => {
            visit_expression_for_calls(&logical.left, mapper, state);
            visit_expression_for_calls(&logical.right, mapper, state);
        }
        Expression::ConditionalExpression(conditional) => {
            visit_expression_for_calls(&conditional.test, mapper, state);
            visit_expression_for_calls(&conditional.consequent, mapper, state);
            visit_expression_for_calls(&conditional.alternate, mapper, state);
        }
        Expression::AssignmentExpression(assignment) => {
            if let Some(inner) = assignment.left.get_expression() {
                visit_expression_for_calls(inner, mapper, state);
            }
            visit_expression_for_calls(&assignment.right, mapper, state);
        }
        Expression::SequenceExpression(sequence) => {
            for expr in &sequence.expressions {
                visit_expression_for_calls(expr, mapper, state);
            }
        }
        Expression::ParenthesizedExpression(parenthesized) => {
            visit_expression_for_calls(&parenthesized.expression, mapper, state);
        }
        Expression::AwaitExpression(await_expr) => {
            visit_expression_for_calls(&await_expr.argument, mapper, state);
        }
        Expression::YieldExpression(yield_expr) => {
            if let Some(argument) = &yield_expr.argument {
                visit_expression_for_calls(argument, mapper, state);
            }
        }
        Expression::NewExpression(new_expression) => {
            visit_expression_for_calls(&new_expression.callee, mapper, state);
            for argument in &new_expression.arguments {
                visit_argument(argument, mapper, state);
            }
        }
        Expression::ChainExpression(chain_expression) => {
            visit_chain_element(&chain_expression.expression, mapper, state);
        }
        Expression::ArrowFunctionExpression(function) => {
            for statement in &function.body.statements {
                visit_statement_for_calls(statement, mapper, state);
            }
        }
        Expression::FunctionExpression(function) => {
            visit_function(function, mapper, state);
        }
        Expression::ClassExpression(class_decl) => {
            visit_class(class_decl, mapper, state);
        }
        Expression::TSAsExpression(ts_as) => {
            visit_expression_for_calls(&ts_as.expression, mapper, state);
        }
        Expression::TSSatisfiesExpression(ts_satisfies) => {
            visit_expression_for_calls(&ts_satisfies.expression, mapper, state);
        }
        Expression::TSTypeAssertion(ts_assertion) => {
            visit_expression_for_calls(&ts_assertion.expression, mapper, state);
        }
        Expression::TSNonNullExpression(ts_non_null) => {
            visit_expression_for_calls(&ts_non_null.expression, mapper, state);
        }
        Expression::TSInstantiationExpression(ts_instantiation) => {
            visit_expression_for_calls(&ts_instantiation.expression, mapper, state);
        }
        Expression::ComputedMemberExpression(member) => {
            visit_expression_for_calls(&member.object, mapper, state);
            visit_expression_for_calls(&member.expression, mapper, state);
        }
        Expression::StaticMemberExpression(member) => {
            visit_expression_for_calls(&member.object, mapper, state);
        }
        Expression::PrivateFieldExpression(member) => {
            visit_expression_for_calls(&member.object, mapper, state);
        }
        _ => {}
    }
}

fn visit_chain_element(
    chain: &ChainElement<'_>,
    mapper: &SourceMapper,
    state: &mut AnalyzerState<'_>,
) {
    match chain {
        ChainElement::CallExpression(call) => {
            collect_call_expression(call, mapper, state);
            visit_expression_for_calls(&call.callee, mapper, state);
            for argument in &call.arguments {
                visit_argument(argument, mapper, state);
            }
        }
        ChainElement::TSNonNullExpression(ts_non_null) => {
            visit_expression_for_calls(&ts_non_null.expression, mapper, state);
        }
        oxc_ast::match_member_expression!(ChainElement) => {
            if let Some(member) = chain.member_expression() {
                visit_expression_for_calls(member.object(), mapper, state);
                if let oxc_ast::ast::MemberExpression::ComputedMemberExpression(computed) = member {
                    visit_expression_for_calls(&computed.expression, mapper, state);
                }
            }
        }
    }
}

fn visit_array_element(
    element: &ArrayExpressionElement<'_>,
    mapper: &SourceMapper,
    state: &mut AnalyzerState<'_>,
) {
    match element {
        ArrayExpressionElement::SpreadElement(spread) => {
            visit_expression_for_calls(&spread.argument, mapper, state)
        }
        _ => {
            if let Some(expression) = element.as_expression() {
                visit_expression_for_calls(expression, mapper, state);
            }
        }
    }
}

fn visit_argument(argument: &Argument<'_>, mapper: &SourceMapper, state: &mut AnalyzerState<'_>) {
    match argument {
        Argument::SpreadElement(spread) => {
            visit_expression_for_calls(&spread.argument, mapper, state)
        }
        _ => {
            if let Some(expression) = argument.as_expression() {
                visit_expression_for_calls(expression, mapper, state);
            }
        }
    }
}

fn collect_call_expression(
    call: &CallExpression<'_>,
    mapper: &SourceMapper,
    state: &mut AnalyzerState<'_>,
) {
    if let Some(require_literal) = call.common_js_require()
        && state.matches_import_pattern(require_literal.value.as_str())
    {
        state.has_envelope_import = true;
    }

    if !matches_call_pattern(call, state) {
        return;
    }

    let Some(first_argument) = call.arguments.first() else {
        return;
    };

    let Some(contract_id) = argument_contract_id(first_argument) else {
        return;
    };

    let (line, column) = mapper.line_col(call.span.start);
    state.calls.push(EnvelopeCall {
        contract_id,
        line,
        column,
        span_start: call.span.start,
        span_end: call.span.end,
    });
}

fn matches_call_pattern(call: &CallExpression<'_>, state: &AnalyzerState<'_>) -> bool {
    if state.pattern_parts.is_empty() {
        return false;
    }

    if state.pattern_parts.len() == 1 {
        let function_name = &state.pattern_parts[0];
        return matches_identifier_name(&call.callee, function_name)
            && state.is_import_binding(function_name);
    }

    let object_name = &state.pattern_parts[0];
    let member_name = &state.pattern_parts[1];

    let Some(member_expr) = call.callee.get_member_expr() else {
        return false;
    };

    if member_expr.static_property_name() != Some(member_name.as_str()) {
        return false;
    }

    if let Some(object_identifier) = expression_identifier_name(member_expr.object()) {
        return state.is_import_binding(object_identifier)
            || (state.has_envelope_import && object_identifier == object_name);
    }

    namespace_object_identifier(member_expr.object(), object_name)
        .is_some_and(|namespace_binding| state.is_namespace_import_binding(namespace_binding))
}

fn namespace_object_identifier<'a>(
    expression: &'a Expression<'a>,
    expected_object_name: &str,
) -> Option<&'a str> {
    let expression = expression.get_inner_expression();

    match expression {
        oxc_ast::match_member_expression!(Expression) => {
            let member = expression.to_member_expression();
            if member.static_property_name() != Some(expected_object_name) {
                return None;
            }

            expression_identifier_name(member.object())
        }
        _ => None,
    }
}

fn matches_identifier_name(expression: &Expression<'_>, expected: &str) -> bool {
    expression_identifier_name(expression).is_some_and(|name| name == expected)
}

fn expression_identifier_name<'a>(expression: &'a Expression<'a>) -> Option<&'a str> {
    match expression.get_inner_expression() {
        Expression::Identifier(identifier) => Some(identifier.name.as_str()),
        Expression::TSNonNullExpression(ts_non_null) => {
            expression_identifier_name(&ts_non_null.expression)
        }
        Expression::TSInstantiationExpression(ts_instantiation) => {
            expression_identifier_name(&ts_instantiation.expression)
        }
        Expression::TSAsExpression(ts_as) => expression_identifier_name(&ts_as.expression),
        Expression::TSTypeAssertion(ts_assertion) => {
            expression_identifier_name(&ts_assertion.expression)
        }
        Expression::TSSatisfiesExpression(ts_satisfies) => {
            expression_identifier_name(&ts_satisfies.expression)
        }
        _ => None,
    }
}

fn argument_contract_id(argument: &Argument<'_>) -> Option<String> {
    let expression = argument.as_expression()?;
    expression_contract_id(expression)
}

fn expression_contract_id(expression: &Expression<'_>) -> Option<String> {
    match expression.get_inner_expression() {
        Expression::StringLiteral(literal) => Some(literal.value.to_string()),
        Expression::TemplateLiteral(template) if template.expressions.is_empty() => Some(
            template
                .quasis
                .iter()
                .map(|quasi| quasi.value.cooked.unwrap_or(quasi.value.raw).to_string())
                .collect(),
        ),
        Expression::TSAsExpression(ts_as) => expression_contract_id(&ts_as.expression),
        _ => None,
    }
}

#[derive(Debug)]
struct SourceMapper {
    line_starts: Vec<usize>,
}

impl SourceMapper {
    fn new(source: &str) -> Self {
        let mut line_starts = vec![0usize];
        for (idx, byte) in source.as_bytes().iter().enumerate() {
            if *byte == b'\n' {
                line_starts.push(idx + 1);
            }
        }
        Self { line_starts }
    }

    fn line_col(&self, offset: u32) -> (usize, usize) {
        let offset = usize::try_from(offset).unwrap_or(0);

        let line_index = match self.line_starts.binary_search(&offset) {
            Ok(exact) => exact,
            Err(insert) => insert.saturating_sub(1),
        };

        let line_start = self.line_starts.get(line_index).copied().unwrap_or(0);
        let line = line_index + 1;
        let column = offset.saturating_sub(line_start);
        (line, column)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    fn run_analysis(source: &str) -> EnvelopeAnalysis {
        run_analysis_with(
            source,
            &["specgate-envelope".to_string()],
            "boundary.validate",
        )
    }

    fn run_analysis_with(
        source: &str,
        import_patterns: &[String],
        function_pattern: &str,
    ) -> EnvelopeAnalysis {
        let temp = TempDir::new().expect("tempdir");
        let file_path = temp.path().join("sample.ts");
        fs::write(&file_path, source).expect("write sample");

        analyze_file_for_envelope(&file_path, import_patterns, function_pattern, None)
            .expect("envelope analysis")
    }

    fn block_span_from_source(source: &str) -> (u32, u32) {
        let start = source.find('{').expect("missing '{'");
        let end = source.rfind('}').expect("missing '}'");

        (start as u32, end as u32 + 1)
    }

    fn build_call(span_start: u32, span_end: u32) -> EnvelopeCall {
        EnvelopeCall {
            contract_id: "create_user".to_string(),
            line: 0,
            column: 0,
            span_start,
            span_end,
        }
    }

    #[test]
    fn finds_exported_function_span_for_exported_function() {
        let source = "export function createUser() {\n  return 1;\n}\n";

        let expected = block_span_from_source(source);
        assert_eq!(
            find_exported_function_span(source, "createUser"),
            Some(expected)
        );
    }

    #[test]
    fn finds_exported_function_span_for_async_exported_function() {
        let source = "export async function createUser() {\n  return 1;\n}\n";

        let expected = block_span_from_source(source);
        assert_eq!(
            find_exported_function_span(source, "createUser"),
            Some(expected)
        );
    }

    #[test]
    fn finds_exported_function_span_for_arrow_exported_const() {
        let source = "export const createUser = () => {\n  return 1;\n};\n";

        let expected = block_span_from_source(source);
        assert_eq!(
            find_exported_function_span(source, "createUser"),
            Some(expected)
        );
    }

    #[test]
    fn finds_exported_function_span_for_async_arrow_exported_const() {
        let source = "export const createUser = async () => {\n  return 1;\n};\n";

        let expected = block_span_from_source(source);
        assert_eq!(
            find_exported_function_span(source, "createUser"),
            Some(expected)
        );
    }

    #[test]
    fn finds_exported_function_span_for_export_default_function() {
        let source = "export default function createUser() {\n  return 1;\n}\n";

        let expected = block_span_from_source(source);
        assert_eq!(
            find_exported_function_span(source, "createUser"),
            Some(expected)
        );
    }

    #[test]
    fn finds_exported_function_span_returns_none_when_not_found() {
        let source = "export function createUser() {\n  return 1;\n}\n";

        assert_eq!(find_exported_function_span(source, "deleteUser"), None);
    }

    #[test]
    fn finds_exported_function_span_only_matches_exported_functions() {
        let source = "function createUser() {\n  return 1;\n}\n";

        assert_eq!(find_exported_function_span(source, "createUser"), None);
    }

    #[test]
    fn finds_exported_function_span_for_hoc_wrapper_export() {
        let source = "import { boundary } from 'specgate-envelope';\n\nexport const createUser = withAuth(() => {\n  boundary.validate('create_user', data);\n});\n";

        let analysis = run_analysis(source);
        let span = find_exported_function_span(source, "createUser").expect("createUser span");
        let calls = filter_calls_by_span(&analysis.calls, span.0, span.1);

        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].contract_id, "create_user");
    }

    #[test]
    fn finds_exported_function_span_for_indirect_export() {
        let source = "import { boundary } from 'specgate-envelope';\n\nconst handler = () => {\n  boundary.validate('id', data);\n};\n\nexport { handler };\n";

        let analysis = run_analysis(source);
        let span = find_exported_function_span(source, "handler").expect("handler span");
        let calls = filter_calls_by_span(&analysis.calls, span.0, span.1);

        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].contract_id, "id");
    }

    #[test]
    fn finds_exported_function_span_for_exported_class_method() {
        let source = "import { boundary } from 'specgate-envelope';\n\nexport class UserService {\n  createUser() {\n    boundary.validate('id', data);\n  }\n}\n";

        let analysis = run_analysis(source);
        let span = find_exported_function_span(source, "createUser").expect("createUser span");
        let calls = filter_calls_by_span(&analysis.calls, span.0, span.1);

        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].contract_id, "id");
    }

    #[test]
    fn hoc_wrapper_span_filters_out_calls_outside_wrapper() {
        let source = "import { boundary } from 'specgate-envelope';\n\nexport const deleteUser = withAuth(() => {\n  console.log('noop');\n});\n\nboundary.validate('outside', data);\n";

        let analysis = run_analysis(source);
        let span = find_exported_function_span(source, "deleteUser").expect("deleteUser span");
        let calls = filter_calls_by_span(&analysis.calls, span.0, span.1);

        assert!(calls.is_empty());
        assert_eq!(analysis.calls.len(), 1);
        assert_eq!(analysis.calls[0].contract_id, "outside");
    }

    #[test]
    fn filter_calls_by_span_includes_inside_calls() {
        let calls = vec![build_call(10, 20), build_call(30, 40)];

        let filtered = filter_calls_by_span(&calls, 5, 25);

        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].span_start, 10);
    }

    #[test]
    fn filter_calls_by_span_excludes_outside_calls() {
        let calls = vec![build_call(10, 20)];

        let filtered = filter_calls_by_span(&calls, 25, 100);

        assert!(filtered.is_empty());
    }

    #[test]
    fn finds_scoped_envelope_call_for_matching_exported_function() {
        let source = "import { boundary } from 'specgate-envelope';\n\nexport function createUser() {\n  boundary.validate('create_user', data);\n}\n\nexport function deleteUser() {\n  console.log('noop');\n}\n";

        let analysis = run_analysis(source);

        let create_span =
            find_exported_function_span(source, "createUser").expect("createUser span");
        let create_calls = filter_calls_by_span(&analysis.calls, create_span.0, create_span.1);

        assert_eq!(create_calls.len(), 1);
        assert_eq!(create_calls[0].contract_id, "create_user");

        let delete_span =
            find_exported_function_span(source, "deleteUser").expect("deleteUser span");
        let delete_calls = filter_calls_by_span(&analysis.calls, delete_span.0, delete_span.1);

        assert!(delete_calls.is_empty());
    }

    #[test]
    fn analyze_file_for_envelope_scopes_calls_when_match_pattern_is_set() {
        let source = "import { boundary } from 'specgate-envelope';\n\nexport function createUser() {\n  return data;\n}\n\nexport function other() {\n  boundary.validate('create_user', data);\n}\n";

        let temp = TempDir::new().expect("tempdir");
        let file_path = temp.path().join("sample.ts");
        fs::write(&file_path, source).expect("write sample");

        let scoped_create = analyze_file_for_envelope(
            &file_path,
            &["specgate-envelope".to_string()],
            "boundary.validate",
            Some("createUser"),
        )
        .expect("envelope analysis");
        assert!(scoped_create.calls.is_empty());

        let scoped_other = analyze_file_for_envelope(
            &file_path,
            &["specgate-envelope".to_string()],
            "boundary.validate",
            Some("other"),
        )
        .expect("envelope analysis");
        assert_eq!(scoped_other.calls.len(), 1);
        assert_eq!(scoped_other.calls[0].contract_id, "create_user");
    }

    #[test]
    fn detects_standard_esm_import_and_call() {
        let analysis = run_analysis(
            "import { boundary } from 'specgate-envelope';\nboundary.validate('create_user', data);\n",
        );

        assert!(analysis.has_envelope_import);
        assert_eq!(analysis.import_bindings, vec!["boundary".to_string()]);
        assert!(!analysis.is_type_only_import);
        assert_eq!(analysis.calls.len(), 1);
        assert_eq!(analysis.calls[0].contract_id, "create_user");
    }

    #[test]
    fn detects_destructured_validate_binding() {
        let analysis = run_analysis_with(
            "import { validate } from 'specgate-envelope';\nvalidate('create_user', data);\n",
            &["specgate-envelope".to_string()],
            "validate",
        );

        assert!(analysis.has_envelope_import);
        assert_eq!(analysis.import_bindings, vec!["validate".to_string()]);
        assert_eq!(analysis.calls.len(), 1);
        assert_eq!(analysis.calls[0].contract_id, "create_user");
    }

    #[test]
    fn detects_renamed_import_binding() {
        let analysis = run_analysis(
            "import { boundary as b } from 'specgate-envelope';\nb.validate('create_user', data);\n",
        );

        assert!(analysis.has_envelope_import);
        assert_eq!(analysis.import_bindings, vec!["b".to_string()]);
        assert_eq!(analysis.calls.len(), 1);
        assert_eq!(analysis.calls[0].contract_id, "create_user");
    }

    #[test]
    fn detects_require_based_import() {
        let analysis =
            run_analysis("require('specgate-envelope');\nboundary.validate('id', data);\n");

        assert!(analysis.has_envelope_import);
        assert!(analysis.import_bindings.is_empty());
        assert_eq!(analysis.calls.len(), 1);
        assert_eq!(analysis.calls[0].contract_id, "id");
    }

    #[test]
    fn mixed_type_only_and_runtime_import_is_treated_as_runtime_import() {
        let analysis = run_analysis(
            "import type { BoundaryType } from 'specgate-envelope';\nimport { boundary } from 'specgate-envelope';\nboundary.validate('create_user', data);\n",
        );

        assert!(analysis.has_envelope_import);
        assert!(!analysis.is_type_only_import);
        assert_eq!(analysis.calls.len(), 1);
        assert_eq!(analysis.calls[0].contract_id, "create_user");
    }

    #[test]
    fn marks_type_only_import_and_ignores_for_runtime() {
        let analysis = run_analysis(
            "import type { boundary } from 'specgate-envelope';\nboundary.validate('create_user', data);\n",
        );

        assert!(!analysis.has_envelope_import);
        assert!(analysis.is_type_only_import);
        assert!(analysis.calls.is_empty());
    }

    #[test]
    fn marks_namespace_import_as_runtime_envelope_import() {
        let analysis = run_analysis(
            "import * as env from 'specgate-envelope';\nenv.boundary.validate('create_user', data);\n",
        );

        assert!(analysis.has_envelope_import);
        assert!(!analysis.is_type_only_import);
    }

    #[test]
    fn detects_namespace_member_call_for_default_boundary_pattern() {
        let analysis = run_analysis(
            "import * as env from 'specgate-envelope';\nenv.boundary.validate('create_user', data);\n",
        );

        assert_eq!(analysis.calls.len(), 1);
        assert_eq!(analysis.calls[0].contract_id, "create_user");
    }

    #[test]
    fn side_effect_import_does_not_create_bindings() {
        let analysis =
            run_analysis("import 'specgate-envelope';\nboundary.validate('create_user', data);\n");

        assert!(!analysis.has_envelope_import);
        assert!(analysis.import_bindings.is_empty());
        assert!(analysis.calls.is_empty());
    }

    #[test]
    fn detects_static_template_literal_contract_id() {
        let analysis = run_analysis(
            "import { boundary } from 'specgate-envelope';\nboundary.validate(`create_user`, data);\n",
        );

        assert_eq!(analysis.calls.len(), 1);
        assert_eq!(analysis.calls[0].contract_id, "create_user");
    }

    #[test]
    fn unwraps_ts_as_expression_for_contract_id() {
        let analysis = run_analysis(
            "import { boundary } from 'specgate-envelope';\nboundary.validate('create_user' as const, data);\n",
        );

        assert_eq!(analysis.calls.len(), 1);
        assert_eq!(analysis.calls[0].contract_id, "create_user");
    }

    #[test]
    fn detects_optional_chaining_call() {
        let analysis = run_analysis(
            "import { boundary } from 'specgate-envelope';\nboundary?.validate('create_user', data);\n",
        );

        assert_eq!(analysis.calls.len(), 1);
        assert_eq!(analysis.calls[0].contract_id, "create_user");
    }

    #[test]
    fn ignores_calls_without_contract_argument() {
        let analysis =
            run_analysis("import { boundary } from 'specgate-envelope';\nboundary.validate();\n");

        assert!(analysis.calls.is_empty());
    }

    #[test]
    fn ignores_wrong_function_name() {
        let analysis = run_analysis(
            "import { boundary } from 'specgate-envelope';\nboundary.schema('create_user', data);\n",
        );

        assert!(analysis.calls.is_empty());
    }

    #[test]
    fn ignores_variable_contract_id() {
        let analysis = run_analysis(
            "import { boundary } from 'specgate-envelope';\nconst id = 'create_user';\nboundary.validate(id, data);\n",
        );

        assert!(analysis.calls.is_empty());
    }

    #[test]
    fn reports_no_import_when_not_present() {
        let analysis = run_analysis("boundary.validate('create_user', data);\n");

        assert!(!analysis.has_envelope_import);
        assert!(analysis.import_bindings.is_empty());
        assert!(analysis.calls.is_empty());
    }

    #[test]
    fn detects_multiple_calls() {
        let analysis = run_analysis(
            "import { boundary } from 'specgate-envelope';\nboundary.validate('create_user', data);\nboundary.validate('delete_user', data);\n",
        );

        assert_eq!(analysis.calls.len(), 2);
        assert_eq!(analysis.calls[0].contract_id, "create_user");
        assert_eq!(analysis.calls[1].contract_id, "delete_user");
    }

    #[test]
    fn supports_custom_import_pattern() {
        let analysis = run_analysis_with(
            "import { validate } from '@myorg/validation';\nvalidate('id', data);\n",
            &["@myorg/validation".to_string()],
            "validate",
        );

        assert!(analysis.has_envelope_import);
        assert_eq!(analysis.import_bindings, vec!["validate".to_string()]);
        assert_eq!(analysis.calls.len(), 1);
        assert_eq!(analysis.calls[0].contract_id, "id");
    }
}
