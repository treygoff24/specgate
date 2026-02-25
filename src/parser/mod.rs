use std::fs;
use std::path::{Path, PathBuf};

use chrono::NaiveDate;
use miette::Diagnostic;
use oxc_allocator::Allocator;
use oxc_ast::ast::{
    Argument, ArrayExpressionElement, CallExpression, ChainElement, Comment, Declaration,
    ExportDefaultDeclarationKind, Expression, ForStatementInit, ForStatementLeft, Function,
    MemberExpression, ObjectPropertyKind, Statement,
};
use oxc_parser::Parser;
use oxc_span::{SourceType, Span};
use thiserror::Error;

use crate::deterministic::stable_unique;

pub mod ignore;

use ignore::parse_ignore_comment;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileAnalysis {
    pub path: PathBuf,
    pub imports: Vec<ImportInfo>,
    pub exports: Vec<ExportInfo>,
    pub re_exports: Vec<ReExportInfo>,
    pub require_calls: Vec<RequireCallInfo>,
    pub dynamic_imports: Vec<DynamicImportInfo>,
    pub dynamic_warnings: Vec<DynamicImportWarning>,
    pub jest_mock_calls: Vec<JestMockCallInfo>,
    pub parse_warnings: Vec<String>,
}

impl FileAnalysis {
    pub fn dependency_specifiers(&self) -> Vec<String> {
        let mut all = Vec::new();
        all.extend(self.imports.iter().map(|i| i.specifier.clone()));
        all.extend(self.re_exports.iter().map(|r| r.specifier.clone()));
        all.extend(self.require_calls.iter().map(|r| r.specifier.clone()));
        all.extend(self.dynamic_imports.iter().map(|d| d.specifier.clone()));
        all.extend(self.jest_mock_calls.iter().map(|j| j.specifier.clone()));
        stable_unique(all)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportInfo {
    pub specifier: String,
    pub is_type_only: bool,
    pub line: u32,
    pub column: u32,
    pub ignore_comment: Option<IgnoreComment>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportInfo {
    pub name: String,
    pub is_type_only: bool,
    pub is_default: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReExportInfo {
    pub specifier: String,
    pub is_star: bool,
    pub names: Vec<String>,
    pub line: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequireCallInfo {
    pub specifier: String,
    pub line: u32,
    pub column: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DynamicImportInfo {
    pub specifier: String,
    pub line: u32,
    pub column: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DynamicImportWarning {
    pub rule: String,
    pub message: String,
    pub line: u32,
    pub column: u32,
    pub expression: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JestMockCallInfo {
    pub specifier: String,
    pub line: u32,
    pub column: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IgnoreComment {
    pub reason: String,
    pub expiry: Option<NaiveDate>,
}

#[derive(Debug, Error, Diagnostic)]
pub enum ParserError {
    #[error("failed to read source file: {path}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

pub type Result<T> = std::result::Result<T, ParserError>;

/// Parse a TS/JS file and extract file-edge dependency data.
///
/// Returns `Err` only for I/O failures.
/// Parse failures become non-fatal warnings on the returned `FileAnalysis`.
pub fn parse_file(path: &Path) -> Result<FileAnalysis> {
    let source = fs::read_to_string(path).map_err(|source| ParserError::Io {
        path: path.to_path_buf(),
        source,
    })?;

    let allocator = Allocator::default();
    let source_type =
        SourceType::from_path(path).unwrap_or_else(|_| SourceType::default().with_module(true));
    let parser_return = Parser::new(&allocator, &source, source_type).parse();

    let mut analysis = FileAnalysis {
        path: path.to_path_buf(),
        imports: Vec::new(),
        exports: Vec::new(),
        re_exports: Vec::new(),
        require_calls: Vec::new(),
        dynamic_imports: Vec::new(),
        dynamic_warnings: Vec::new(),
        jest_mock_calls: Vec::new(),
        parse_warnings: parser_return
            .errors
            .iter()
            .map(ToString::to_string)
            .collect(),
    };

    let mapper = SourceMapper::new(&source);

    extract_module_declarations(
        &parser_return.program.body,
        parser_return.program.comments.as_slice(),
        &source,
        &mapper,
        &mut analysis,
    );

    for dynamic_import in &parser_return.module_record.dynamic_imports {
        let expr_span = dynamic_import.module_request;
        let (line, column) = mapper.line_col(expr_span.start);
        let raw_expression = span_slice(&source, expr_span)
            .unwrap_or_default()
            .trim()
            .to_string();

        if let Some(specifier) = parse_string_literal_expression(&raw_expression) {
            analysis.dynamic_imports.push(DynamicImportInfo {
                specifier,
                line,
                column,
            });
        } else {
            analysis.dynamic_warnings.push(DynamicImportWarning {
                rule: "resolver.unresolved_dynamic_import".to_string(),
                message: "dynamic import uses a non-literal expression".to_string(),
                line,
                column,
                expression: raw_expression,
            });
        }
    }

    for statement in &parser_return.program.body {
        visit_statement_for_calls(statement, &source, &mapper, &mut analysis);
    }

    sort_analysis(&mut analysis);
    Ok(analysis)
}

fn extract_module_declarations(
    statements: &[Statement<'_>],
    comments: &[Comment],
    source: &str,
    mapper: &SourceMapper,
    analysis: &mut FileAnalysis,
) {
    for statement in statements {
        match statement {
            Statement::ImportDeclaration(decl) => {
                let (line, column) = mapper.line_col(decl.span.start);
                let ignore_comment = find_ignore_comment(comments, source, decl.span, mapper);

                analysis.imports.push(ImportInfo {
                    specifier: decl.source.value.to_string(),
                    is_type_only: is_import_type_only(decl),
                    line,
                    column,
                    ignore_comment,
                });
            }
            Statement::ExportAllDeclaration(decl) => {
                let (line, _column) = mapper.line_col(decl.span.start);
                analysis.re_exports.push(ReExportInfo {
                    specifier: decl.source.value.to_string(),
                    is_star: true,
                    names: Vec::new(),
                    line,
                });
            }
            Statement::ExportNamedDeclaration(decl) => {
                if let Some(source_literal) = &decl.source {
                    let (line, _column) = mapper.line_col(decl.span.start);
                    let names = decl
                        .specifiers
                        .iter()
                        .map(|specifier| specifier.exported.name().to_string())
                        .collect();
                    analysis.re_exports.push(ReExportInfo {
                        specifier: source_literal.value.to_string(),
                        is_star: false,
                        names,
                        line,
                    });
                } else {
                    for specifier in &decl.specifiers {
                        analysis.exports.push(ExportInfo {
                            name: specifier.exported.name().to_string(),
                            is_type_only: decl.export_kind.is_type()
                                || specifier.export_kind.is_type(),
                            is_default: false,
                        });
                    }

                    if let Some(declaration) = &decl.declaration {
                        extract_declaration_exports(declaration, analysis);
                    }
                }
            }
            Statement::ExportDefaultDeclaration(_) => {
                analysis.exports.push(ExportInfo {
                    name: "__default".to_string(),
                    is_type_only: false,
                    is_default: true,
                });
            }
            _ => {}
        }
    }
}

fn extract_declaration_exports(declaration: &Declaration<'_>, analysis: &mut FileAnalysis) {
    match declaration {
        Declaration::FunctionDeclaration(function) => {
            if let Some(id) = &function.id {
                analysis.exports.push(ExportInfo {
                    name: id.name.to_string(),
                    is_type_only: false,
                    is_default: false,
                });
            }
        }
        Declaration::ClassDeclaration(class_decl) => {
            if let Some(id) = &class_decl.id {
                analysis.exports.push(ExportInfo {
                    name: id.name.to_string(),
                    is_type_only: false,
                    is_default: false,
                });
            }
        }
        _ => {}
    }
}

fn visit_statement_for_calls(
    statement: &Statement<'_>,
    source: &str,
    mapper: &SourceMapper,
    analysis: &mut FileAnalysis,
) {
    match statement {
        Statement::BlockStatement(block) => {
            for nested in &block.body {
                visit_statement_for_calls(nested, source, mapper, analysis);
            }
        }
        Statement::ExpressionStatement(expr_stmt) => {
            visit_expression_for_calls(&expr_stmt.expression, source, mapper, analysis);
        }
        Statement::DoWhileStatement(stmt) => {
            visit_statement_for_calls(&stmt.body, source, mapper, analysis);
            visit_expression_for_calls(&stmt.test, source, mapper, analysis);
        }
        Statement::WhileStatement(stmt) => {
            visit_expression_for_calls(&stmt.test, source, mapper, analysis);
            visit_statement_for_calls(&stmt.body, source, mapper, analysis);
        }
        Statement::ForStatement(stmt) => {
            if let Some(init) = &stmt.init {
                visit_for_init(init, source, mapper, analysis);
            }
            if let Some(test) = &stmt.test {
                visit_expression_for_calls(test, source, mapper, analysis);
            }
            if let Some(update) = &stmt.update {
                visit_expression_for_calls(update, source, mapper, analysis);
            }
            visit_statement_for_calls(&stmt.body, source, mapper, analysis);
        }
        Statement::ForInStatement(stmt) => {
            visit_for_left(&stmt.left, source, mapper, analysis);
            visit_expression_for_calls(&stmt.right, source, mapper, analysis);
            visit_statement_for_calls(&stmt.body, source, mapper, analysis);
        }
        Statement::ForOfStatement(stmt) => {
            visit_for_left(&stmt.left, source, mapper, analysis);
            visit_expression_for_calls(&stmt.right, source, mapper, analysis);
            visit_statement_for_calls(&stmt.body, source, mapper, analysis);
        }
        Statement::IfStatement(stmt) => {
            visit_expression_for_calls(&stmt.test, source, mapper, analysis);
            visit_statement_for_calls(&stmt.consequent, source, mapper, analysis);
            if let Some(alternate) = &stmt.alternate {
                visit_statement_for_calls(alternate, source, mapper, analysis);
            }
        }
        Statement::ReturnStatement(stmt) => {
            if let Some(argument) = &stmt.argument {
                visit_expression_for_calls(argument, source, mapper, analysis);
            }
        }
        Statement::SwitchStatement(stmt) => {
            visit_expression_for_calls(&stmt.discriminant, source, mapper, analysis);
            for case in &stmt.cases {
                if let Some(test) = &case.test {
                    visit_expression_for_calls(test, source, mapper, analysis);
                }
                for nested in &case.consequent {
                    visit_statement_for_calls(nested, source, mapper, analysis);
                }
            }
        }
        Statement::ThrowStatement(stmt) => {
            visit_expression_for_calls(&stmt.argument, source, mapper, analysis);
        }
        Statement::TryStatement(stmt) => {
            for nested in &stmt.block.body {
                visit_statement_for_calls(nested, source, mapper, analysis);
            }
            if let Some(handler) = &stmt.handler {
                for nested in &handler.body.body {
                    visit_statement_for_calls(nested, source, mapper, analysis);
                }
            }
            if let Some(finalizer) = &stmt.finalizer {
                for nested in &finalizer.body {
                    visit_statement_for_calls(nested, source, mapper, analysis);
                }
            }
        }
        Statement::WithStatement(stmt) => {
            visit_expression_for_calls(&stmt.object, source, mapper, analysis);
            visit_statement_for_calls(&stmt.body, source, mapper, analysis);
        }
        Statement::LabeledStatement(stmt) => {
            visit_statement_for_calls(&stmt.body, source, mapper, analysis);
        }
        Statement::VariableDeclaration(declaration) => {
            for declarator in &declaration.declarations {
                if let Some(init) = &declarator.init {
                    visit_expression_for_calls(init, source, mapper, analysis);
                }
            }
        }
        Statement::FunctionDeclaration(function) => {
            visit_function(function, source, mapper, analysis);
        }
        Statement::ClassDeclaration(class_decl) => {
            visit_class(class_decl, source, mapper, analysis);
        }
        Statement::ExportNamedDeclaration(decl) => {
            if let Some(inner_declaration) = &decl.declaration {
                visit_declaration(inner_declaration, source, mapper, analysis);
            }
        }
        Statement::ExportDefaultDeclaration(decl) => {
            visit_export_default_kind(&decl.declaration, source, mapper, analysis);
        }
        _ => {}
    }
}

fn visit_declaration(
    declaration: &Declaration<'_>,
    source: &str,
    mapper: &SourceMapper,
    analysis: &mut FileAnalysis,
) {
    match declaration {
        Declaration::VariableDeclaration(declaration) => {
            for declarator in &declaration.declarations {
                if let Some(init) = &declarator.init {
                    visit_expression_for_calls(init, source, mapper, analysis);
                }
            }
        }
        Declaration::FunctionDeclaration(function) => {
            visit_function(function, source, mapper, analysis);
        }
        Declaration::ClassDeclaration(class_decl) => {
            visit_class(class_decl, source, mapper, analysis);
        }
        _ => {}
    }
}

fn visit_export_default_kind(
    kind: &ExportDefaultDeclarationKind<'_>,
    source: &str,
    mapper: &SourceMapper,
    analysis: &mut FileAnalysis,
) {
    match kind {
        ExportDefaultDeclarationKind::FunctionDeclaration(function) => {
            visit_function(function, source, mapper, analysis)
        }
        ExportDefaultDeclarationKind::ClassDeclaration(class_decl) => {
            visit_class(class_decl, source, mapper, analysis)
        }
        _ => {
            if let Some(expr) = kind.as_expression() {
                visit_expression_for_calls(expr, source, mapper, analysis);
            }
        }
    }
}

fn visit_function(
    function: &Function<'_>,
    source: &str,
    mapper: &SourceMapper,
    analysis: &mut FileAnalysis,
) {
    if let Some(body) = &function.body {
        for statement in &body.statements {
            visit_statement_for_calls(statement, source, mapper, analysis);
        }
    }
}

fn visit_class(
    class_decl: &oxc_ast::ast::Class<'_>,
    source: &str,
    mapper: &SourceMapper,
    analysis: &mut FileAnalysis,
) {
    if let Some(super_class) = &class_decl.super_class {
        visit_expression_for_calls(super_class, source, mapper, analysis);
    }

    for element in &class_decl.body.body {
        match element {
            oxc_ast::ast::ClassElement::StaticBlock(block) => {
                for statement in &block.body {
                    visit_statement_for_calls(statement, source, mapper, analysis);
                }
            }
            oxc_ast::ast::ClassElement::MethodDefinition(method) => {
                visit_function(&method.value, source, mapper, analysis);
            }
            oxc_ast::ast::ClassElement::PropertyDefinition(property) => {
                if let Some(value) = &property.value {
                    visit_expression_for_calls(value, source, mapper, analysis);
                }
            }
            oxc_ast::ast::ClassElement::AccessorProperty(property) => {
                if let Some(value) = &property.value {
                    visit_expression_for_calls(value, source, mapper, analysis);
                }
            }
            _ => {}
        }
    }
}

fn visit_for_init(
    init: &ForStatementInit<'_>,
    source: &str,
    mapper: &SourceMapper,
    analysis: &mut FileAnalysis,
) {
    match init {
        ForStatementInit::VariableDeclaration(declaration) => {
            for declarator in &declaration.declarations {
                if let Some(expr) = &declarator.init {
                    visit_expression_for_calls(expr, source, mapper, analysis);
                }
            }
        }
        _ => {
            if let Some(expr) = init.as_expression() {
                visit_expression_for_calls(expr, source, mapper, analysis);
            }
        }
    }
}

fn visit_for_left(
    left: &ForStatementLeft<'_>,
    source: &str,
    mapper: &SourceMapper,
    analysis: &mut FileAnalysis,
) {
    match left {
        ForStatementLeft::VariableDeclaration(declaration) => {
            for declarator in &declaration.declarations {
                if let Some(expr) = &declarator.init {
                    visit_expression_for_calls(expr, source, mapper, analysis);
                }
            }
        }
        _ => {
            if let Some(target) = left.as_assignment_target() {
                if let Some(expr) = target.get_expression() {
                    visit_expression_for_calls(expr, source, mapper, analysis);
                }
            }
        }
    }
}

fn visit_expression_for_calls(
    expression: &Expression<'_>,
    source: &str,
    mapper: &SourceMapper,
    analysis: &mut FileAnalysis,
) {
    match expression {
        Expression::CallExpression(call) => {
            collect_call_expression(call, mapper, analysis);

            visit_expression_for_calls(&call.callee, source, mapper, analysis);
            for argument in &call.arguments {
                visit_argument(argument, source, mapper, analysis);
            }
        }
        Expression::ImportExpression(import_expression) => {
            visit_expression_for_calls(&import_expression.source, source, mapper, analysis);
            if let Some(options) = &import_expression.options {
                visit_expression_for_calls(options, source, mapper, analysis);
            }
        }
        Expression::ArrayExpression(array_expression) => {
            for element in &array_expression.elements {
                visit_array_element(element, source, mapper, analysis);
            }
        }
        Expression::ObjectExpression(object_expression) => {
            for property in &object_expression.properties {
                match property {
                    ObjectPropertyKind::ObjectProperty(property) => {
                        if property.computed {
                            visit_property_key(&property.key, source, mapper, analysis);
                        }
                        visit_expression_for_calls(&property.value, source, mapper, analysis);
                    }
                    ObjectPropertyKind::SpreadProperty(spread) => {
                        visit_expression_for_calls(&spread.argument, source, mapper, analysis);
                    }
                }
            }
        }
        Expression::TemplateLiteral(template_literal) => {
            for expr in &template_literal.expressions {
                visit_expression_for_calls(expr, source, mapper, analysis);
            }
        }
        Expression::TaggedTemplateExpression(tagged) => {
            visit_expression_for_calls(&tagged.tag, source, mapper, analysis);
            for expr in &tagged.quasi.expressions {
                visit_expression_for_calls(expr, source, mapper, analysis);
            }
        }
        Expression::UnaryExpression(unary) => {
            visit_expression_for_calls(&unary.argument, source, mapper, analysis);
        }
        Expression::UpdateExpression(update) => {
            if let Some(expr) = update.argument.get_expression() {
                visit_expression_for_calls(expr, source, mapper, analysis);
            }
        }
        Expression::BinaryExpression(binary) => {
            visit_expression_for_calls(&binary.left, source, mapper, analysis);
            visit_expression_for_calls(&binary.right, source, mapper, analysis);
        }
        Expression::LogicalExpression(logical) => {
            visit_expression_for_calls(&logical.left, source, mapper, analysis);
            visit_expression_for_calls(&logical.right, source, mapper, analysis);
        }
        Expression::ConditionalExpression(conditional) => {
            visit_expression_for_calls(&conditional.test, source, mapper, analysis);
            visit_expression_for_calls(&conditional.consequent, source, mapper, analysis);
            visit_expression_for_calls(&conditional.alternate, source, mapper, analysis);
        }
        Expression::AssignmentExpression(assignment) => {
            if let Some(inner) = assignment.left.get_expression() {
                visit_expression_for_calls(inner, source, mapper, analysis);
            }
            visit_expression_for_calls(&assignment.right, source, mapper, analysis);
        }
        Expression::SequenceExpression(sequence) => {
            for expr in &sequence.expressions {
                visit_expression_for_calls(expr, source, mapper, analysis);
            }
        }
        Expression::ParenthesizedExpression(parenthesized) => {
            visit_expression_for_calls(&parenthesized.expression, source, mapper, analysis);
        }
        Expression::AwaitExpression(await_expr) => {
            visit_expression_for_calls(&await_expr.argument, source, mapper, analysis);
        }
        Expression::YieldExpression(yield_expr) => {
            if let Some(argument) = &yield_expr.argument {
                visit_expression_for_calls(argument, source, mapper, analysis);
            }
        }
        Expression::NewExpression(new_expression) => {
            visit_expression_for_calls(&new_expression.callee, source, mapper, analysis);
            for argument in &new_expression.arguments {
                visit_argument(argument, source, mapper, analysis);
            }
        }
        Expression::ChainExpression(chain_expression) => {
            visit_chain_element(&chain_expression.expression, source, mapper, analysis);
        }
        Expression::ArrowFunctionExpression(function) => {
            for statement in &function.body.statements {
                visit_statement_for_calls(statement, source, mapper, analysis);
            }
        }
        Expression::FunctionExpression(function) => {
            visit_function(function, source, mapper, analysis);
        }
        Expression::ClassExpression(class_decl) => {
            visit_class(class_decl, source, mapper, analysis);
        }
        Expression::TSAsExpression(ts_as) => {
            visit_expression_for_calls(&ts_as.expression, source, mapper, analysis);
        }
        Expression::TSSatisfiesExpression(ts_satisfies) => {
            visit_expression_for_calls(&ts_satisfies.expression, source, mapper, analysis);
        }
        Expression::TSTypeAssertion(ts_assertion) => {
            visit_expression_for_calls(&ts_assertion.expression, source, mapper, analysis);
        }
        Expression::TSNonNullExpression(ts_non_null) => {
            visit_expression_for_calls(&ts_non_null.expression, source, mapper, analysis);
        }
        Expression::TSInstantiationExpression(ts_instantiation) => {
            visit_expression_for_calls(&ts_instantiation.expression, source, mapper, analysis);
        }
        Expression::ComputedMemberExpression(member) => {
            visit_expression_for_calls(&member.object, source, mapper, analysis);
            visit_expression_for_calls(&member.expression, source, mapper, analysis);
        }
        Expression::StaticMemberExpression(member) => {
            visit_expression_for_calls(&member.object, source, mapper, analysis);
        }
        Expression::PrivateFieldExpression(member) => {
            visit_expression_for_calls(&member.object, source, mapper, analysis);
        }
        _ => {}
    }
}

fn visit_chain_element(
    chain: &ChainElement<'_>,
    source: &str,
    mapper: &SourceMapper,
    analysis: &mut FileAnalysis,
) {
    match chain {
        ChainElement::CallExpression(call) => {
            collect_call_expression(call, mapper, analysis);
            visit_expression_for_calls(&call.callee, source, mapper, analysis);
            for argument in &call.arguments {
                visit_argument(argument, source, mapper, analysis);
            }
        }
        ChainElement::TSNonNullExpression(ts_non_null) => {
            visit_expression_for_calls(&ts_non_null.expression, source, mapper, analysis);
        }
        oxc_ast::match_member_expression!(ChainElement) => {
            if let Some(member) = chain.member_expression() {
                visit_expression_for_calls(member.object(), source, mapper, analysis);
                if let MemberExpression::ComputedMemberExpression(computed) = member {
                    visit_expression_for_calls(&computed.expression, source, mapper, analysis);
                }
            }
        }
    }
}

fn visit_array_element(
    element: &ArrayExpressionElement<'_>,
    source: &str,
    mapper: &SourceMapper,
    analysis: &mut FileAnalysis,
) {
    match element {
        ArrayExpressionElement::SpreadElement(spread) => {
            visit_expression_for_calls(&spread.argument, source, mapper, analysis)
        }
        _ => {
            if let Some(expression) = element.as_expression() {
                visit_expression_for_calls(expression, source, mapper, analysis);
            }
        }
    }
}

fn visit_property_key(
    key: &oxc_ast::ast::PropertyKey<'_>,
    source: &str,
    mapper: &SourceMapper,
    analysis: &mut FileAnalysis,
) {
    if let Some(expression) = key.as_expression() {
        visit_expression_for_calls(expression, source, mapper, analysis);
    }
}

fn visit_argument(
    argument: &Argument<'_>,
    source: &str,
    mapper: &SourceMapper,
    analysis: &mut FileAnalysis,
) {
    match argument {
        Argument::SpreadElement(spread) => {
            visit_expression_for_calls(&spread.argument, source, mapper, analysis)
        }
        _ => {
            if let Some(expression) = argument.as_expression() {
                visit_expression_for_calls(expression, source, mapper, analysis);
            }
        }
    }
}

fn collect_call_expression(
    call: &CallExpression<'_>,
    mapper: &SourceMapper,
    analysis: &mut FileAnalysis,
) {
    if let Some(require_literal) = call.common_js_require() {
        let (line, column) = mapper.line_col(require_literal.span.start);
        analysis.require_calls.push(RequireCallInfo {
            specifier: require_literal.value.to_string(),
            line,
            column,
        });
    }

    if is_jest_mock_call(call)
        && let Some(Argument::StringLiteral(specifier)) = call.arguments.first()
    {
        let (line, column) = mapper.line_col(specifier.span.start);
        analysis.jest_mock_calls.push(JestMockCallInfo {
            specifier: specifier.value.to_string(),
            line,
            column,
        });
    }
}

fn is_jest_mock_call(call: &CallExpression<'_>) -> bool {
    let Some(member) = call.callee.get_member_expr() else {
        return false;
    };

    if member.static_property_name() != Some("mock") {
        return false;
    }

    matches!(member.object(), Expression::Identifier(identifier) if identifier.name == "jest")
}

fn is_import_type_only(declaration: &oxc_ast::ast::ImportDeclaration<'_>) -> bool {
    if declaration.import_kind.is_type() {
        return true;
    }

    declaration.specifiers.as_ref().is_some_and(|specifiers| {
        !specifiers.is_empty()
            && specifiers.iter().all(|specifier| match specifier {
                oxc_ast::ast::ImportDeclarationSpecifier::ImportSpecifier(specifier) => {
                    specifier.import_kind.is_type()
                }
                _ => false,
            })
    })
}

fn find_ignore_comment(
    comments: &[Comment],
    source: &str,
    declaration_span: Span,
    mapper: &SourceMapper,
) -> Option<IgnoreComment> {
    let declaration_line = mapper.line_col(declaration_span.start).0;

    comments
        .iter()
        .filter(|comment| comment.span.end <= declaration_span.start)
        .filter_map(|comment| {
            let comment_end_line = mapper.line_col(comment.span.end).0;
            let distance = declaration_line.saturating_sub(comment_end_line);
            if distance > 1 {
                return None;
            }

            let raw_comment = span_slice(source, comment.span)?;
            let parsed = parse_ignore_comment(raw_comment)?;
            Some((comment.span.end, parsed))
        })
        .max_by_key(|(end, _)| *end)
        .map(|(_, parsed)| parsed)
}

fn parse_string_literal_expression(expr: &str) -> Option<String> {
    let trimmed = expr.trim();
    if trimmed.len() < 2 {
        return None;
    }

    let starts_with_quote = trimmed.starts_with('"') || trimmed.starts_with('\'');
    let ends_with_quote = trimmed.ends_with('"') || trimmed.ends_with('\'');

    if starts_with_quote && ends_with_quote {
        let inner = &trimmed[1..trimmed.len() - 1];
        return Some(inner.to_string());
    }

    None
}

fn sort_analysis(analysis: &mut FileAnalysis) {
    analysis.imports.sort_by(|a, b| {
        a.line
            .cmp(&b.line)
            .then_with(|| a.column.cmp(&b.column))
            .then_with(|| a.specifier.cmp(&b.specifier))
    });

    analysis.re_exports.sort_by(|a, b| {
        a.line
            .cmp(&b.line)
            .then_with(|| a.specifier.cmp(&b.specifier))
            .then_with(|| a.names.cmp(&b.names))
    });

    analysis.exports.sort_by(|a, b| {
        a.name
            .cmp(&b.name)
            .then_with(|| a.is_default.cmp(&b.is_default))
    });

    analysis.require_calls.sort_by(|a, b| {
        a.line
            .cmp(&b.line)
            .then_with(|| a.column.cmp(&b.column))
            .then_with(|| a.specifier.cmp(&b.specifier))
    });

    analysis.dynamic_imports.sort_by(|a, b| {
        a.line
            .cmp(&b.line)
            .then_with(|| a.column.cmp(&b.column))
            .then_with(|| a.specifier.cmp(&b.specifier))
    });

    analysis.dynamic_warnings.sort_by(|a, b| {
        a.line
            .cmp(&b.line)
            .then_with(|| a.column.cmp(&b.column))
            .then_with(|| a.expression.cmp(&b.expression))
    });

    analysis.jest_mock_calls.sort_by(|a, b| {
        a.line
            .cmp(&b.line)
            .then_with(|| a.column.cmp(&b.column))
            .then_with(|| a.specifier.cmp(&b.specifier))
    });
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

    fn line_col(&self, offset: u32) -> (u32, u32) {
        let offset = usize::try_from(offset).unwrap_or(0);

        let line_index = match self.line_starts.binary_search(&offset) {
            Ok(exact) => exact,
            Err(insert) => insert.saturating_sub(1),
        };

        let line_start = self.line_starts.get(line_index).copied().unwrap_or(0);
        let line = u32::try_from(line_index + 1).unwrap_or(u32::MAX);
        let column = u32::try_from(offset.saturating_sub(line_start)).unwrap_or(u32::MAX);
        (line, column)
    }
}

fn span_slice(source: &str, span: Span) -> Option<&str> {
    let start = usize::try_from(span.start).ok()?;
    let end = usize::try_from(span.end).ok()?;
    source.get(start..end)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    #[test]
    fn extracts_import_export_and_call_edges() {
        let temp = TempDir::new().expect("tempdir");
        let file_path = temp.path().join("sample.ts");

        fs::write(
            &file_path,
            r#"
// @specgate-ignore: legacy edge
import type { Foo } from "./types";
import { bar } from "./bar";

export { bar as baz };
export * from "./all";
export { qux } from "./qux";

const dep = require("left-pad");
async function load(name: string) {
  await import("./lazy");
  await import(name);
}

jest.mock("./mocked");
export default function main() {}
"#,
        )
        .expect("write sample");

        let analysis = parse_file(&file_path).expect("analysis");

        assert_eq!(analysis.imports.len(), 2);
        assert!(analysis.imports[0].is_type_only);
        assert!(analysis.imports[0].ignore_comment.is_some());

        assert!(
            analysis
                .re_exports
                .iter()
                .any(|edge| edge.specifier == "./all" && edge.is_star)
        );
        assert!(
            analysis
                .re_exports
                .iter()
                .any(|edge| edge.specifier == "./qux" && !edge.is_star)
        );

        assert!(
            analysis
                .require_calls
                .iter()
                .any(|edge| edge.specifier == "left-pad")
        );

        assert!(
            analysis
                .dynamic_imports
                .iter()
                .any(|edge| edge.specifier == "./lazy")
        );
        assert!(
            analysis
                .dynamic_warnings
                .iter()
                .any(|warning| warning.rule == "resolver.unresolved_dynamic_import")
        );

        assert!(
            analysis
                .jest_mock_calls
                .iter()
                .any(|edge| edge.specifier == "./mocked")
        );

        assert!(analysis.exports.iter().any(|export| export.is_default));
    }

    #[test]
    fn parse_errors_are_non_fatal() {
        let temp = TempDir::new().expect("tempdir");
        let file_path = temp.path().join("broken.ts");
        fs::write(&file_path, "import { from './x'\n").expect("write broken file");

        let analysis = parse_file(&file_path).expect("analysis");
        assert!(!analysis.parse_warnings.is_empty());
    }

    #[test]
    fn ignore_parser_rejects_non_ignore_comments() {
        assert!(parse_ignore_comment("// regular note").is_none());
    }
}
