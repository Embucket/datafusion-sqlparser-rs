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

//! Snowflake warehouse syntax. Parsing grants no execution or account authority.

#[cfg(not(feature = "std"))]
use alloc::{string::String, vec::Vec};
use core::fmt;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};
#[cfg(feature = "visitor")]
use sqlparser_derive::{Visit, VisitMut};

use crate::ast::{display_comma_separated, DescribeAlias, Expr, Ident, ObjectName, Value};

/// An ordered warehouse property. Duplicate names remain available for semantic validation.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "visitor", derive(Visit, VisitMut))]
pub struct WarehouseOption {
    /// Property name, without a parser-owned semantic whitelist.
    pub name: Ident,
    /// Parsed primitive identifier, literal, or signed numeric literal; never evaluated.
    /// Consumers must revalidate shapes after external AST construction or mutation.
    pub value: Expr,
}

impl fmt::Display for WarehouseOption {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} = {}", self.name, self.value)
    }
}

/// `CREATE [OR REPLACE] WAREHOUSE [IF NOT EXISTS] name [[WITH] properties]`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "visitor", derive(Visit, VisitMut))]
pub struct CreateWarehouse {
    /// Whether `OR REPLACE` was present; conflicting clauses are retained for the semantic owner.
    pub or_replace: bool,
    /// Whether `IF NOT EXISTS` was present.
    pub if_not_exists: bool,
    /// Account object name. Multipart and generating names are not resolved by parsing.
    pub name: ObjectName,
    /// Whether properties were introduced by `WITH`.
    pub with_keyword: bool,
    /// Ordered properties, including duplicates and unsupported semantic values.
    pub options: Vec<WarehouseOption>,
}

impl fmt::Display for CreateWarehouse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CREATE")?;
        if self.or_replace {
            write!(f, " OR REPLACE")?;
        }
        write!(f, " WAREHOUSE")?;
        if self.if_not_exists {
            write!(f, " IF NOT EXISTS")?;
        }
        write!(f, " {}", self.name)?;
        if self.with_keyword {
            write!(f, " WITH")?;
        }
        for option in &self.options {
            write!(f, " {option}")?;
        }
        Ok(())
    }
}

/// A selected `ALTER WAREHOUSE` action, independent of execution support.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "visitor", derive(Visit, VisitMut))]
pub enum AlterWarehouseAction {
    /// Suspend the named warehouse or the current warehouse when no name is supplied.
    Suspend,
    /// Resume a warehouse, optionally succeeding when it is already started.
    Resume {
        /// Whether `IF SUSPENDED` was present.
        if_suspended: bool,
    },
    /// Set properties in their original order, including duplicates.
    Set(Vec<WarehouseOption>),
    /// Unset the listed property names.
    Unset(Vec<Ident>),
    /// Rename to an unresolved account object name.
    RenameTo(ObjectName),
}

impl fmt::Display for AlterWarehouseAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Suspend => write!(f, "SUSPEND"),
            Self::Resume { if_suspended } => {
                write!(f, "RESUME")?;
                if *if_suspended {
                    write!(f, " IF SUSPENDED")?;
                }
                Ok(())
            }
            Self::Set(options) => {
                write!(f, "SET")?;
                for option in options {
                    write!(f, " {option}")?;
                }
                Ok(())
            }
            Self::Unset(names) => write!(f, "UNSET {}", display_comma_separated(names)),
            Self::RenameTo(name) => write!(f, "RENAME TO {name}"),
        }
    }
}

/// `ALTER WAREHOUSE [IF EXISTS] [name] action`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "visitor", derive(Visit, VisitMut))]
pub struct AlterWarehouse {
    /// Whether `IF EXISTS` was present.
    pub if_exists: bool,
    /// Omitted only for the selected SUSPEND/RESUME grammar.
    pub name: Option<ObjectName>,
    /// The parsed action. No statement is executed while constructing the AST.
    pub action: AlterWarehouseAction,
}

impl fmt::Display for AlterWarehouse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ALTER WAREHOUSE")?;
        if self.if_exists {
            write!(f, " IF EXISTS")?;
        }
        if let Some(name) = &self.name {
            write!(f, " {name}")?;
        }
        write!(f, " {}", self.action)
    }
}

/// `SHOW [TERSE] WAREHOUSES` with selected literal filters.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "visitor", derive(Visit, VisitMut))]
pub struct ShowWarehouses {
    /// Whether `TERSE` was present; does not assert a particular result schema.
    pub terse: bool,
    /// Case-insensitive SQL LIKE pattern, interpreted only by a future execution consumer.
    pub like_pattern: Option<String>,
    /// Selected reference grammar; filter semantics are not inferred by the parser.
    pub starts_with: Option<String>,
    /// Numeric LIMIT syntax; bounds are owned by semantic validation.
    pub limit: Option<Expr>,
}

impl fmt::Display for ShowWarehouses {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SHOW")?;
        if self.terse {
            write!(f, " TERSE")?;
        }
        write!(f, " WAREHOUSES")?;
        if let Some(pattern) = &self.like_pattern {
            write!(f, " LIKE {}", Value::SingleQuotedString(pattern.clone()))?;
        }
        if let Some(prefix) = &self.starts_with {
            write!(
                f,
                " STARTS WITH {}",
                Value::SingleQuotedString(prefix.clone())
            )?;
        }
        if let Some(limit) = &self.limit {
            write!(f, " LIMIT {limit}")?;
        }
        Ok(())
    }
}

/// `DESC[RIBE] WAREHOUSE name`, distinct from table description.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "visitor", derive(Visit, VisitMut))]
pub struct DescribeWarehouse {
    /// The original DESC/DESCRIBE spelling.
    pub describe_alias: DescribeAlias,
    /// Unresolved account object name, without table-relation visitor annotations.
    pub name: ObjectName,
}

impl fmt::Display for DescribeWarehouse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} WAREHOUSE {}", self.describe_alias, self.name)
    }
}

/// `DROP WAREHOUSE [IF EXISTS] name`, without generic DROP modifiers or multiple targets.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "visitor", derive(Visit, VisitMut))]
pub struct DropWarehouse {
    /// Whether `IF EXISTS` was present.
    pub if_exists: bool,
    /// Unresolved account object name, without table-relation visitor annotations.
    pub name: ObjectName,
}

impl fmt::Display for DropWarehouse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "DROP WAREHOUSE")?;
        if self.if_exists {
            write!(f, " IF EXISTS")?;
        }
        write!(f, " {}", self.name)
    }
}
