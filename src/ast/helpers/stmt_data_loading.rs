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

//! AST types specific to loading and unloading syntax, like one available in Snowflake which
//! contains: STAGE ddl operations, PUT upload or COPY INTO
//! See [this page](https://docs.snowflake.com/en/sql-reference/commands-data-loading) for more details.

#[cfg(not(feature = "std"))]
use alloc::{format, string::String, vec::Vec};
use core::fmt;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use crate::ast::helpers::key_value_options::KeyValueOptions;
use crate::ast::{Expr, Ident, ObjectName, SelectItem, UnaryOperator, Value};
#[cfg(feature = "visitor")]
use sqlparser_derive::{Visit, VisitMut};

/// Snowflake `PUT` upload syntax. Execution and transfer admission are separate concerns.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "visitor", derive(Visit, VisitMut))]
pub struct Put {
    /// Local source URI or a binding expression.
    pub source: FileTransferLocation,
    /// Stage destination or a binding expression.
    pub target: FileTransferLocation,
    /// Ordered options, including duplicates for owning semantic validation.
    pub options: Vec<FileTransferOption>,
}

/// Snowflake `GET` download syntax.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "visitor", derive(Visit, VisitMut))]
pub struct Get {
    /// Stage source or a binding expression.
    pub source: FileTransferLocation,
    /// Local destination URI or a binding expression.
    pub target: FileTransferLocation,
    /// Ordered options, including duplicates for owning semantic validation.
    pub options: Vec<FileTransferOption>,
}

/// A transfer operand, distinct from a table relation and from arbitrary SQL text.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "visitor", derive(Visit, VisitMut))]
pub enum FileTransferLocation {
    /// A local URI, preserving the `file://` prefix and path bytes.
    Local {
        /// Exact URI; the parser does not resolve or normalize a filesystem path.
        uri: String,
        /// Whether the complete operand was enclosed in single quotes.
        quoted: bool,
    },
    /// A typed stage reference and separate optional payload prefix.
    Stage(FileTransferStageLocation),
    /// A native SQL placeholder. Visitors must consume it through normal expression binding.
    Binding(Expr),
}

/// A stage reference used by transfer syntax, without table-relation visitor annotations.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "visitor", derive(Visit, VisitMut))]
pub struct FileTransferStageLocation {
    /// Named, user, or table stage syntax. Parsing does not imply execution support.
    pub kind: FileTransferStage,
    /// Exact bytes after the first slash outside quoted identifiers; `Some("")` retains `/`.
    pub prefix: Option<String>,
    /// Whether the complete operand was enclosed in single quotes.
    pub quoted: bool,
}

/// Stage kinds must remain distinct so an execution layer can explicitly refuse unsupported ones.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "visitor", derive(Visit, VisitMut))]
pub enum FileTransferStage {
    /// A real qualified object name, retaining every identifier's quoting.
    Named(ObjectName),
    /// The current user's `@~` stage.
    User,
    /// A table's `@[namespace.]%table` stage, retaining its qualified name.
    Table(ObjectName),
}

/// One transfer option. Values use normal expression/value nodes, not enum-like placeholders.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "visitor", derive(Visit, VisitMut))]
pub struct FileTransferOption {
    /// Original option name, including its identifier quoting.
    pub name: Ident,
    /// Typed value traversed by normal expression binding.
    pub value: Expr,
    /// Original primitive numeric spelling when the numeric AST feature normalizes it.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub number_spelling: Option<String>,
}

pub(crate) fn file_transfer_number_matches(value: &Expr, raw: &str) -> bool {
    let (value, digits) = match value {
        Expr::UnaryOp {
            op: UnaryOperator::Plus,
            expr,
        } => {
            let Some(digits) = raw.strip_prefix('+') else {
                return false;
            };
            (expr.as_ref(), digits)
        }
        Expr::UnaryOp {
            op: UnaryOperator::Minus,
            expr,
        } => {
            let Some(digits) = raw.strip_prefix('-') else {
                return false;
            };
            (expr.as_ref(), digits)
        }
        _ if raw.starts_with(['+', '-']) => return false,
        _ => (value, raw),
    };
    let Expr::Value(value) = value else {
        return false;
    };
    let Ok(number) = digits.parse() else {
        return false;
    };
    value.value == Value::Number(number, false)
}

impl fmt::Display for FileTransferStage {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Named(name) => write!(f, "{name}"),
            Self::User => f.write_str("~"),
            Self::Table(name) => {
                for (index, part) in name.0.iter().enumerate() {
                    if index > 0 {
                        f.write_str(".")?;
                    }
                    if index + 1 == name.0.len() {
                        f.write_str("%")?;
                    }
                    write!(f, "{part}")?;
                }
                Ok(())
            }
        }
    }
}

impl fmt::Display for FileTransferStageLocation {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut value = format!("@{}", self.kind);
        if let Some(prefix) = &self.prefix {
            value.push('/');
            value.push_str(prefix);
        }
        display_transfer_location(f, &value, self.quoted)
    }
}

fn display_transfer_location(f: &mut fmt::Formatter, value: &str, quoted: bool) -> fmt::Result {
    if quoted || file_transfer_location_needs_quotes(value) {
        // Transfer paths keep backslashes literal; only the enclosing SQL quote is escaped.
        write!(f, "'{}'", value.replace('\'', "''"))
    } else {
        f.write_str(value)
    }
}

pub(crate) fn file_transfer_location_needs_quotes(value: &str) -> bool {
    let mut stage_name = value.starts_with('@');
    let mut identifier_quote = false;
    let mut chars = value.chars().peekable();
    while let Some(ch) = chars.next() {
        if stage_name && ch == '"' {
            if identifier_quote && chars.peek() == Some(&'"') {
                chars.next();
            } else {
                identifier_quote = !identifier_quote;
            }
        } else if stage_name && ch == '/' && !identifier_quote {
            stage_name = false;
        } else if !identifier_quote && (ch.is_whitespace() || ch == ';') {
            return true;
        }
    }
    false
}

impl fmt::Display for FileTransferLocation {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Local { uri, quoted } => display_transfer_location(f, uri, *quoted),
            Self::Stage(stage) => write!(f, "{stage}"),
            Self::Binding(expr) => write!(f, "{expr}"),
        }
    }
}

impl fmt::Display for FileTransferOption {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}=", self.name)?;
        if let Some(raw) = self
            .number_spelling
            .as_deref()
            .filter(|raw| file_transfer_number_matches(&self.value, raw))
        {
            f.write_str(raw)
        } else {
            write!(f, "{}", self.value)
        }
    }
}

impl fmt::Display for Put {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "PUT {} {}", self.source, self.target)?;
        for option in &self.options {
            write!(f, " {option}")?;
        }
        Ok(())
    }
}

impl fmt::Display for Get {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "GET {} {}", self.source, self.target)?;
        for option in &self.options {
            write!(f, " {option}")?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "visitor", derive(Visit, VisitMut))]
/// Parameters for a named stage object used in data loading/unloading.
pub struct StageParamsObject {
    /// Optional URL for the stage.
    pub url: Option<String>,
    /// Encryption-related key/value options.
    pub encryption: KeyValueOptions,
    /// Optional endpoint string.
    pub endpoint: Option<String>,
    /// Optional storage integration identifier.
    pub storage_integration: Option<String>,
    /// Credentials for accessing the stage.
    pub credentials: KeyValueOptions,
}

impl StageParamsObject {
    /// Returns true when no stage parameter is present.
    pub fn is_empty(&self) -> bool {
        self.url.is_none()
            && self.encryption.options.is_empty()
            && self.endpoint.is_none()
            && self.storage_integration.is_none()
            && self.credentials.options.is_empty()
    }
}

/// An operation supported by Snowflake's `ALTER STAGE` statement.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "visitor", derive(Visit, VisitMut))]
pub enum AlterStageOperation {
    /// Rename a stage.
    RenameTo {
        /// New stage name.
        #[cfg_attr(feature = "visitor", visit(with = "visit_relation"))]
        new_name: ObjectName,
    },
    /// Set one or more stage properties.
    Set {
        /// External stage parameters.
        stage_params: StageParamsObject,
        /// Directory table parameters.
        directory_table_params: KeyValueOptions,
        /// File format options.
        file_format: KeyValueOptions,
        /// Copy options.
        copy_options: KeyValueOptions,
        /// Optional comment.
        comment: Option<String>,
    },
}

/// An operation supported by Snowflake's `ALTER FILE FORMAT` statement.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "visitor", derive(Visit, VisitMut))]
pub enum AlterFileFormatOperation {
    /// Rename a named file format.
    RenameTo {
        /// New file format name.
        #[cfg_attr(feature = "visitor", visit(with = "visit_relation"))]
        new_name: ObjectName,
    },
    /// Set one or more format-specific properties or the comment.
    Set {
        /// Format-specific options.
        options: KeyValueOptions,
        /// Optional comment replacement.
        comment: Option<String>,
    },
}

/// This enum enables support for both standard SQL select item expressions
/// and Snowflake-specific ones for data loading.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "visitor", derive(Visit, VisitMut))]
pub enum StageLoadSelectItemKind {
    /// A standard SQL select item expression.
    SelectItem(SelectItem),
    /// A Snowflake-specific select item used for stage loading.
    StageLoadSelectItem(StageLoadSelectItem),
}

impl fmt::Display for StageLoadSelectItemKind {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match &self {
            StageLoadSelectItemKind::SelectItem(item) => write!(f, "{item}"),
            StageLoadSelectItemKind::StageLoadSelectItem(item) => write!(f, "{item}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "visitor", derive(Visit, VisitMut))]
/// A single item in the `SELECT` list for data loading from staged files.
pub struct StageLoadSelectItem {
    /// Optional alias for the input source.
    pub alias: Option<Ident>,
    /// Column number within the staged file (1-based).
    pub file_col_num: i32,
    /// Original spelling when the staged placeholder differs from its numeric form.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub file_col_num_raw: Option<String>,
    /// Optional element identifier following the column reference.
    pub element: Option<Ident>,
    /// Optional alias for the item (AS clause).
    pub item_as: Option<Ident>,
}

impl fmt::Display for StageParamsObject {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let url = &self.url.as_ref();
        let storage_integration = &self.storage_integration.as_ref();
        let endpoint = &self.endpoint.as_ref();

        if url.is_some() {
            write!(f, " URL='{}'", url.unwrap())?;
        }
        if storage_integration.is_some() {
            write!(f, " STORAGE_INTEGRATION={}", storage_integration.unwrap())?;
        }
        if endpoint.is_some() {
            write!(f, " ENDPOINT='{}'", endpoint.unwrap())?;
        }
        if !self.credentials.options.is_empty() {
            write!(f, " CREDENTIALS=({})", self.credentials)?;
        }
        if !self.encryption.options.is_empty() {
            write!(f, " ENCRYPTION=({})", self.encryption)?;
        }

        Ok(())
    }
}

impl fmt::Display for StageLoadSelectItem {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if let Some(alias) = &self.alias {
            write!(f, "{alias}.")?;
        }
        if let Some(raw) = self.file_col_num_raw.as_deref().filter(|raw| {
            raw.strip_prefix('$')
                .and_then(|digits| digits.parse::<i32>().ok())
                == Some(self.file_col_num)
        }) {
            write!(f, "{raw}")?;
        } else {
            write!(f, "${}", self.file_col_num)?;
        }
        if let Some(element) = &self.element {
            write!(f, ":{element}")?;
        }
        if let Some(item_as) = &self.item_as {
            write!(f, " AS {item_as}")?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, PartialOrd, Eq, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "visitor", derive(Visit, VisitMut))]
/// A command to stage files to a named stage.
pub struct FileStagingCommand {
    /// The stage to which files are being staged.
    #[cfg_attr(feature = "visitor", visit(with = "visit_relation"))]
    pub stage: ObjectName,
    /// Optional file matching `PATTERN` expression.
    pub pattern: Option<String>,
}

impl fmt::Display for FileStagingCommand {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.stage)?;
        if let Some(pattern) = self.pattern.as_ref() {
            write!(f, " PATTERN='{pattern}'")?;
        }
        Ok(())
    }
}
