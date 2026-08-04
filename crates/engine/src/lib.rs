use oxc_allocator::Allocator;
use oxc_ast::ast::{BinaryExpression, Expression, LogicalExpression, ReturnStatement};
use oxc_ast_visit::{walk, Visit};
use oxc_parser::{config::TokensParserConfig, Kind, Parser, Token};
use oxc_span::{GetSpan, SourceType};
use oxc_syntax::operator::{BinaryOperator, LogicalOperator};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum Operator {
    CondBoundaryGt,
    CondBoundaryLt,
    LogicalAndToOr,
    LogicalOrToAnd,
    EqualityStrictToLooseNeg,
    InequalityToEquality,
    ReturnTrueToFalse,
    ReturnFalseToTrue,
}

impl Operator {
    pub const ALL: [Self; 8] = [
        Self::CondBoundaryGt,
        Self::CondBoundaryLt,
        Self::LogicalAndToOr,
        Self::LogicalOrToAnd,
        Self::EqualityStrictToLooseNeg,
        Self::InequalityToEquality,
        Self::ReturnTrueToFalse,
        Self::ReturnFalseToTrue,
    ];

    pub const fn id(self) -> &'static str {
        match self {
            Self::CondBoundaryGt => "cond-boundary-gt",
            Self::CondBoundaryLt => "cond-boundary-lt",
            Self::LogicalAndToOr => "logical-and-to-or",
            Self::LogicalOrToAnd => "logical-or-to-and",
            Self::EqualityStrictToLooseNeg => "equality-strict-to-loose-neg",
            Self::InequalityToEquality => "inequality-to-equality",
            Self::ReturnTrueToFalse => "return-true-to-false",
            Self::ReturnFalseToTrue => "return-false-to-true",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|operator| operator.id() == id)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Mutant {
    pub line: u32,
    pub column: u32,
    pub operator: Operator,
    pub span_start: u32,
    pub span_end: u32,
    pub replacement: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParseFailure {
    pub path: String,
    pub diagnostics: Vec<String>,
}

impl std::fmt::Display for ParseFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "failed to parse {} as TypeScript: {}",
            self.path,
            self.diagnostics.join("; ")
        )
    }
}

impl std::error::Error for ParseFailure {}

pub fn try_mutants(
    source: &str,
    path: &str,
    operators: &[Operator],
) -> Result<Vec<Mutant>, ParseFailure> {
    let allocator = Allocator::default();
    let source_type = SourceType::from_path(path).unwrap_or_else(|_| SourceType::ts());
    let parsed = Parser::new(&allocator, source, source_type)
        .with_config(TokensParserConfig)
        .parse();
    if !parsed.diagnostics.is_empty() {
        return Err(ParseFailure {
            path: path.to_owned(),
            diagnostics: parsed
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.to_string())
                .collect(),
        });
    }

    let mut finder = Finder {
        source,
        tokens: parsed.tokens.as_slice(),
        enabled: operators,
        found: Vec::new(),
    };
    finder.visit_program(&parsed.program);
    finder
        .found
        .sort_by_key(|mutant| (mutant.span_start, mutant.operator));
    Ok(finder.found)
}

pub fn mutants(source: &str, path: &str, operators: &[Operator]) -> Vec<Mutant> {
    try_mutants(source, path, operators).unwrap_or_default()
}

pub fn apply(source: &str, mutant: &Mutant) -> String {
    let start = mutant.span_start as usize;
    let end = mutant.span_end as usize;
    let Some(_) = source.get(start..end) else {
        return source.to_owned();
    };

    let mut result = String::with_capacity(source.len() + mutant.replacement.len());
    result.push_str(&source[..start]);
    result.push_str(&mutant.replacement);
    result.push_str(&source[end..]);
    result
}

struct Finder<'source> {
    source: &'source str,
    tokens: &'source [Token],
    enabled: &'source [Operator],
    found: Vec<Mutant>,
}

impl<'ast> Visit<'ast> for Finder<'_> {
    fn visit_binary_expression(&mut self, expression: &BinaryExpression<'ast>) {
        if let Some((operator, expected, replacement)) = binary_mutation(expression.operator) {
            self.record_between(
                operator,
                expected,
                replacement,
                expression.left.span().end,
                expression.right.span().start,
            );
        }
        walk::walk_binary_expression(self, expression);
    }

    fn visit_logical_expression(&mut self, expression: &LogicalExpression<'ast>) {
        if let Some((operator, expected, replacement)) = logical_mutation(expression.operator) {
            self.record_between(
                operator,
                expected,
                replacement,
                expression.left.span().end,
                expression.right.span().start,
            );
        }
        walk::walk_logical_expression(self, expression);
    }

    fn visit_return_statement(&mut self, statement: &ReturnStatement<'ast>) {
        if let Some(Expression::BooleanLiteral(literal)) = statement.argument.as_ref() {
            let (operator, expected, replacement) = return_mutation(literal.value);
            self.record_between(
                operator,
                expected,
                replacement,
                literal.span.start,
                literal.span.end,
            );
        }
        walk::walk_return_statement(self, statement);
    }
}

impl Finder<'_> {
    fn record_between(
        &mut self,
        operator: Operator,
        expected: Kind,
        replacement: &str,
        start: u32,
        end: u32,
    ) {
        if !self.enabled.contains(&operator) {
            return;
        }
        let Some(token) = self
            .tokens
            .iter()
            .find(|token| token.kind() == expected && token.start() >= start && token.end() <= end)
        else {
            return;
        };
        let (line, column) = position_at(self.source, token.start() as usize);
        self.found.push(Mutant {
            line,
            column,
            operator,
            span_start: token.start(),
            span_end: token.end(),
            replacement: replacement.to_owned(),
        });
    }
}

fn binary_mutation(operator: BinaryOperator) -> Option<(Operator, Kind, &'static str)> {
    match operator {
        BinaryOperator::GreaterThan => Some((Operator::CondBoundaryGt, Kind::RAngle, ">=")),
        BinaryOperator::LessThan => Some((Operator::CondBoundaryLt, Kind::LAngle, "<=")),
        BinaryOperator::StrictEquality => {
            Some((Operator::EqualityStrictToLooseNeg, Kind::Eq3, "!=="))
        }
        BinaryOperator::StrictInequality => {
            Some((Operator::InequalityToEquality, Kind::Neq2, "==="))
        }
        _ => None,
    }
}

fn logical_mutation(operator: LogicalOperator) -> Option<(Operator, Kind, &'static str)> {
    match operator {
        LogicalOperator::And => Some((Operator::LogicalAndToOr, Kind::Amp2, "||")),
        LogicalOperator::Or => Some((Operator::LogicalOrToAnd, Kind::Pipe2, "&&")),
        _ => None,
    }
}

fn return_mutation(value: bool) -> (Operator, Kind, &'static str) {
    if value {
        (Operator::ReturnTrueToFalse, Kind::True, "false")
    } else {
        (Operator::ReturnFalseToTrue, Kind::False, "true")
    }
}

fn position_at(source: &str, offset: usize) -> (u32, u32) {
    let prefix = &source[..offset];
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count() as u32 + 1;
    let column = prefix
        .rsplit('\n')
        .next()
        .unwrap_or_default()
        .chars()
        .count() as u32
        + 1;
    (line, column)
}

#[cfg(test)]
mod tests {
    use super::{apply, mutants, Operator};

    fn all_operators() -> [Operator; 8] {
        Operator::ALL
    }

    fn mutated_output(source: &str, operator: Operator) -> String {
        let found = mutants(source, "fixture.ts", &[operator]);
        assert_eq!(found.len(), 1);
        apply(source, &found[0])
    }

    #[test]
    fn finds_each_supported_operator() {
        let source = "const gt = left > right;\nconst lt = left < right;\nconst and = left && right;\nconst or = left || right;\nconst equality = left === right;\nconst inequality = left !== right;\nfunction values() { return true; return false; }\n";
        let found = mutants(source, "fixture.ts", &all_operators());
        let operators: Vec<_> = found.iter().map(|mutant| mutant.operator).collect();
        let replacements: Vec<_> = found
            .iter()
            .map(|mutant| mutant.replacement.as_str())
            .collect();
        assert_eq!(operators, Operator::ALL);
        assert_eq!(
            replacements,
            vec![">=", "<=", "||", "&&", "!==", "===", "false", "true"]
        );
        assert_eq!(
            found.iter().map(|mutant| mutant.line).collect::<Vec<_>>(),
            vec![1, 2, 3, 4, 5, 6, 7, 7]
        );
    }

    #[test]
    fn mutates_cond_boundary_gt() {
        let source = "const ready = left > right;\n";
        assert_eq!(
            mutated_output(source, Operator::CondBoundaryGt),
            "const ready = left >= right;\n"
        );
    }

    #[test]
    fn mutates_cond_boundary_lt() {
        let source = "const ready = left < right;\n";
        assert_eq!(
            mutated_output(source, Operator::CondBoundaryLt),
            "const ready = left <= right;\n"
        );
    }

    #[test]
    fn mutates_logical_and_to_or() {
        let source = "const ready = left && right;\n";
        assert_eq!(
            mutated_output(source, Operator::LogicalAndToOr),
            "const ready = left || right;\n"
        );
    }

    #[test]
    fn mutates_logical_or_to_and() {
        let source = "const ready = left || right;\n";
        assert_eq!(
            mutated_output(source, Operator::LogicalOrToAnd),
            "const ready = left && right;\n"
        );
    }

    #[test]
    fn mutates_strict_equality_to_strict_inequality() {
        let source = "const ready = left === right;\n";
        assert_eq!(
            mutated_output(source, Operator::EqualityStrictToLooseNeg),
            "const ready = left !== right;\n"
        );
    }

    #[test]
    fn mutates_strict_inequality_to_strict_equality() {
        let source = "const ready = left !== right;\n";
        assert_eq!(
            mutated_output(source, Operator::InequalityToEquality),
            "const ready = left === right;\n"
        );
    }

    #[test]
    fn mutates_return_true_to_false() {
        let source = "function ready() { return true; }\n";
        assert_eq!(
            mutated_output(source, Operator::ReturnTrueToFalse),
            "function ready() { return false; }\n"
        );
    }

    #[test]
    fn mutates_return_false_to_true() {
        let source = "function ready() { return false; }\n";
        assert_eq!(
            mutated_output(source, Operator::ReturnFalseToTrue),
            "function ready() { return true; }\n"
        );
    }

    #[test]
    fn ignores_operator_text_in_strings_and_comments() {
        let source = "const text = \"> < && || === !== return true; return false;\";\n// > < && || === !== return true; return false;\n/* > < && || === !== return true; return false; */\nconst gt = left > right;\nconst lt = left < right;\nconst and = left && right;\nconst or = left || right;\nconst equality = left === right;\nconst inequality = left !== right;\nfunction values() { return true; return false; }\n";
        let found = mutants(source, "fixture.ts", &all_operators());
        assert_eq!(found.len(), Operator::ALL.len());
        assert_eq!(
            found
                .iter()
                .map(|mutant| mutant.operator)
                .collect::<Vec<_>>(),
            Operator::ALL
        );
        assert!(found.iter().all(|mutant| mutant.line >= 4));
    }

    #[test]
    fn apply_replaces_only_the_ast_selected_operator() {
        let source = "const ready = left === right;\n";
        let found = mutants(source, "fixture.ts", &[Operator::EqualityStrictToLooseNeg]);
        assert_eq!(apply(source, &found[0]), "const ready = left !== right;\n");
    }

    #[test]
    fn results_are_stably_sorted_by_span() {
        let source = "const first = alpha && beta;\nconst second = gamma === delta;\nconst third = epsilon || zeta;\n";
        let first = mutants(source, "fixture.ts", &all_operators());
        let second = mutants(source, "fixture.ts", &all_operators());
        let starts: Vec<_> = first.iter().map(|mutant| mutant.span_start).collect();
        assert_eq!(first, second);
        assert!(starts.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn does_not_mutate_inclusive_comparisons() {
        let source = "const greater = left >= right;\nconst lesser = left <= right;\n";
        let found = mutants(
            source,
            "fixture.ts",
            &[Operator::CondBoundaryGt, Operator::CondBoundaryLt],
        );
        assert!(found.is_empty());
    }

    #[test]
    fn finds_return_in_nested_arrow_function() {
        let source =
            "const outer = () => { const nested = () => { return true; }; return false; };\n";
        let found = mutants(source, "fixture.ts", &[Operator::ReturnTrueToFalse]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].line, 1);
        assert_eq!(
            apply(source, &found[0]),
            "const outer = () => { const nested = () => { return false; }; return false; };\n"
        );
    }

    #[test]
    fn does_not_mutate_nonliteral_return() {
        let source = "function value(x) { return x; }\n";
        let found = mutants(
            source,
            "fixture.ts",
            &[Operator::ReturnTrueToFalse, Operator::ReturnFalseToTrue],
        );
        assert!(found.is_empty());
    }
}
