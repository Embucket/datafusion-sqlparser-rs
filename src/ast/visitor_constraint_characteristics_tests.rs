// Licensed to the Apache Software Foundation (ASF) under one
// or more contributor license agreements.  See the NOTICE file
// distributed with this work for additional information
// regarding copyright ownership.  The ASF licenses this file
// to you under the Apache License, Version 2.0 (the
// "License"); you may not use this file except in compliance
// with the License.  You may obtain a copy of the License at
//
//   http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing,
// software distributed under the License is distributed on an
// "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied.  See the License for the
// specific language governing permissions and limitations
// under the License.

use super::*;
use crate::ast::{
    ColumnOption, ColumnOptions, ConstraintUsingIndex, CreateDomain, HiveDistributionStyle, Ident,
    SetExpr, TableConstraint, ViewColumnDef,
};
use crate::dialect::{GenericDialect, PostgreSqlDialect};
use crate::parser::{Parser, ParserError};

type TestError = Box<dyn std::error::Error>;
type TestResult = Result<(), TestError>;
type NativeContainer = (&'static str, Statement, usize);

fn statement(sql: &str) -> Result<Statement, ParserError> {
    Ok(Parser::parse_sql(&GenericDialect {}, sql)?.remove(0))
}

// Native combinations need not be emitted by a dialect's grammar. Every derived
// placement must expose its actual characteristics to a consumer's visitor.
fn native_containers() -> Result<Vec<NativeContainer>, TestError> {
    let inline = statement("CREATE TABLE t(id INT PRIMARY KEY NOT ENFORCED RELY)")?;
    let Statement::CreateTable(template) = &inline else {
        return Err("unexpected native AST fixture".into());
    };
    let column = template.columns[0].clone();
    let mut view = statement("CREATE VIEW v AS SELECT 1")?;
    let Statement::CreateView(create) = &mut view else {
        return Err("unexpected native AST fixture".into());
    };
    create.columns.push(ViewColumnDef {
        name: Ident::new("id"),
        data_type: None,
        options: Some(ColumnOptions::SpaceSeparated(vec![column.options[0]
            .option
            .clone()])),
    });
    let mut partitioned = template.clone();
    partitioned.columns.clear();
    partitioned.hive_distribution = HiveDistributionStyle::PARTITIONED {
        columns: vec![column.clone()],
    };
    let mut skewed = template.clone();
    skewed.columns.clear();
    skewed.hive_distribution = HiveDistributionStyle::SKEWED {
        columns: vec![column.clone()],
        on: vec![column.clone()],
        stored_as_directories: false,
    };
    let Statement::Query(mut query) = statement("SELECT 1")? else {
        return Err("unexpected native AST fixture".into());
    };
    query.body = Box::new(SetExpr::Insert(view.clone()));
    let mut nested = template.clone();
    nested.columns.clear();
    nested.query = Some(query);
    let domain = Statement::CreateDomain(CreateDomain {
        name: vec![Ident::new("d")].into(),
        data_type: column.data_type.clone(),
        collation: None,
        default: None,
        constraints: vec![TableConstraint::PrimaryKeyUsingIndex(
            ConstraintUsingIndex {
                name: None,
                index_name: Ident::new("i"),
                characteristics: Some(ConstraintCharacteristics {
                    rely: Some(true),
                    ..Default::default()
                }),
            },
        )],
    });
    let mut trigger = Parser::parse_sql(
        &PostgreSqlDialect {},
        "CREATE CONSTRAINT TRIGGER trg BEFORE INSERT ON t DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION f",
    )?
    .remove(0);
    let Statement::CreateTrigger(create) = &mut trigger else {
        return Err("unexpected native AST fixture".into());
    };
    create.characteristics.as_mut().unwrap().rely = Some(true);
    Ok(vec![
        ("inline primary key", inline, 1),
        (
            "unique and foreign key",
            statement("CREATE TABLE t(u INT UNIQUE NOT ENFORCED NORELY, f INT REFERENCES other(id) NOT ENFORCED RELY)")?,
            2,
        ),
        (
            "table primary key",
            statement("CREATE TABLE t(id INT, PRIMARY KEY(id) NOT ENFORCED RELY)")?,
            1,
        ),
        ("view column options", view, 1),
        ("Hive partition columns", Statement::CreateTable(partitioned), 1),
        ("Hive skew columns and ON", Statement::CreateTable(skewed), 2),
        ("domain constraint using index", domain, 1),
        ("trigger characteristics", trigger, 1),
        ("nested native DDL", Statement::CreateTable(nested), 1),
    ])
}

#[derive(Default)]
struct Counts {
    pre: Vec<Option<bool>>,
    post: Vec<Option<bool>>,
}

impl Visitor for Counts {
    type Break = ();

    fn pre_visit_constraint_characteristics(
        &mut self,
        value: &ConstraintCharacteristics,
    ) -> ControlFlow<Self::Break> {
        self.pre.push(value.rely);
        ControlFlow::Continue(())
    }

    fn post_visit_constraint_characteristics(
        &mut self,
        value: &ConstraintCharacteristics,
    ) -> ControlFlow<Self::Break> {
        self.post.push(value.rely);
        ControlFlow::Continue(())
    }
}

#[test]
fn constraint_characteristics_hooks_reach_all_native_containers() -> TestResult {
    for (name, node, expected) in native_containers()? {
        let mut counts = Counts::default();
        assert!(Visit::visit(&node, &mut counts).is_continue(), "{name}");
        assert_eq!(counts.pre.len(), expected, "{name}");
        assert_eq!(counts.post, counts.pre, "{name}");
        assert!(counts.pre.iter().all(Option::is_some), "{name}");
    }
    Ok(())
}

#[test]
fn constraint_characteristics_break_preserves_actual_borrowed_identity() -> TestResult {
    let node = statement("CREATE TABLE t(id INT PRIMARY KEY NOT ENFORCED RELY)")?;
    let Statement::CreateTable(create) = &node else {
        return Err("unexpected native AST fixture".into());
    };
    let ColumnOption::PrimaryKey(key) = &create.columns[0].options[0].option else {
        return Err("unexpected native AST fixture".into());
    };
    struct Permit<'a>(&'a ConstraintCharacteristics);
    impl Visitor for Permit<'_> {
        type Break = &'static str;
        fn pre_visit_constraint_characteristics(
            &mut self,
            value: &ConstraintCharacteristics,
        ) -> ControlFlow<Self::Break> {
            if std::ptr::eq(self.0, value) {
                ControlFlow::Continue(())
            } else {
                ControlFlow::Break("another instance")
            }
        }
    }
    let permitted = key.characteristics.as_ref().unwrap();
    assert!(Visit::visit(&node, &mut Permit(permitted)).is_continue());
    assert_eq!(
        Visit::visit(&node.clone(), &mut Permit(permitted)),
        ControlFlow::Break("another instance")
    );
    Ok(())
}

#[test]
fn constraint_characteristics_mutable_hooks_and_post_break_propagate() -> TestResult {
    struct Mutate {
        pre: usize,
        post: usize,
    }
    impl VisitorMut for Mutate {
        type Break = ();
        fn pre_visit_constraint_characteristics(
            &mut self,
            value: &mut ConstraintCharacteristics,
        ) -> ControlFlow<Self::Break> {
            self.pre += 1;
            value.rely = Some(false);
            ControlFlow::Continue(())
        }
        fn post_visit_constraint_characteristics(
            &mut self,
            value: &mut ConstraintCharacteristics,
        ) -> ControlFlow<Self::Break> {
            self.post += 1;
            assert_eq!(value.rely, Some(false));
            ControlFlow::Break(())
        }
    }
    let mut node = statement(
        "CREATE TABLE t(a INT UNIQUE NOT ENFORCED RELY, b INT UNIQUE NOT ENFORCED RELY)",
    )?;
    let mut visitor = Mutate { pre: 0, post: 0 };
    assert!(VisitMut::visit(&mut node, &mut visitor).is_break());
    assert_eq!((visitor.pre, visitor.post), (1, 1));
    let mut after = Counts::default();
    let _ = Visit::visit(&node, &mut after);
    assert_eq!(after.pre, [Some(false), Some(true)]);
    Ok(())
}

#[test]
fn constraint_characteristics_default_hooks_preserve_existing_visitors() -> TestResult {
    #[derive(Default, PartialEq, Debug)]
    struct Existing {
        statements: usize,
        expressions: usize,
    }
    impl Visitor for Existing {
        type Break = ();
        fn pre_visit_statement(&mut self, _: &Statement) -> ControlFlow<Self::Break> {
            self.statements += 1;
            ControlFlow::Continue(())
        }
        fn pre_visit_expr(&mut self, _: &Expr) -> ControlFlow<Self::Break> {
            self.expressions += 1;
            ControlFlow::Continue(())
        }
    }
    let plain = statement("CREATE TABLE t(id INT PRIMARY KEY)")?;
    let reliant = statement("CREATE TABLE t(id INT PRIMARY KEY NOT ENFORCED RELY)")?;
    let (mut before, mut after) = (Existing::default(), Existing::default());
    assert!(Visit::visit(&plain, &mut before).is_continue());
    assert!(Visit::visit(&reliant, &mut after).is_continue());
    assert_eq!(before, after);
    let query = statement("SELECT 1 FROM t WHERE id = 2")?;
    let mut counts = Counts::default();
    assert!(Visit::visit(&query, &mut counts).is_continue());
    assert!(counts.pre.is_empty() && counts.post.is_empty());
    Ok(())
}
