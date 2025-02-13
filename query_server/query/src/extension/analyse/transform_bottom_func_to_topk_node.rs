use std::sync::Arc;

use datafusion::common::tree_node::{Transformed, TreeNode};
use datafusion::config::ConfigOptions;
use datafusion::error::{DataFusionError, Result};
use datafusion::logical_expr::{expr, LogicalPlan, Projection, Sort};
use datafusion::optimizer::analyzer::AnalyzerRule;
use datafusion::prelude::Expr;
use datafusion::scalar::ScalarValue;

use crate::extension::expr::{expr_utils, BOTTOM};

const INVALID_EXPRS: &str = "1. There cannot be nested selection functions. 2. There cannot be multiple selection functions.";
const INVALID_ARGUMENTS: &str =
    "Routine not match. Maybe (field_name, k). k is integer literal value. The range of values for k is [1, 255].";

pub struct TransformBottomFuncToTopkNodeRule {}

impl AnalyzerRule for TransformBottomFuncToTopkNodeRule {
    fn analyze(&self, plan: LogicalPlan, _config: &ConfigOptions) -> Result<LogicalPlan> {
        plan.transform_up(&analyze_internal)
    }

    fn name(&self) -> &str {
        "transform_bottom_func_to_topk_node"
    }
}

fn analyze_internal(plan: LogicalPlan) -> Result<Transformed<LogicalPlan>> {
    if let LogicalPlan::Projection(projection) = &plan {
        // check exprs and then do transform
        if let (true, Some(bottom_function)) = (
            //check exprs
            valid_exprs(&projection.expr)?,
            // extract bottom function expr, If it does not exist, return None
            extract_bottom_function(&projection.expr),
        ) {
            return Ok(Transformed::Yes(do_transform(
                &bottom_function,
                projection,
            )?));
        };
    }

    Ok(Transformed::No(plan))
}

fn do_transform(bottom_function: &Expr, projection: &Projection) -> Result<LogicalPlan> {
    let Projection {
        expr,
        input,
        schema,
        ..
    } = projection;

    let (field, k) = extract_args(bottom_function)?;

    let sort_expr = Expr::Sort(expr::Sort {
        /// The expression to sort on
        expr: Box::new(field.clone()),
        /// The direction of the sort
        asc: true,
        /// Whether to put Nulls before all other data values
        nulls_first: false,
    });

    let topk_node = LogicalPlan::Sort(Sort {
        expr: vec![sort_expr],
        input: input.clone(),
        fetch: Some(k),
    });

    // 2. construct a new projection node
    // * replace bottom func expression with inner column expr
    // * not construct the new set of required columns
    let new_projection = LogicalPlan::Projection(Projection::try_new_with_schema(
        expr_utils::replace_expr_with(expr, bottom_function, &field),
        Arc::new(topk_node),
        schema.clone(),
    )?);

    // 3. Assemble the new execution plan return
    Ok(new_projection)
}

fn valid_exprs(exprs: &[Expr]) -> Result<bool> {
    let selector_function_num = expr_utils::find_selector_function_exprs(exprs).len();
    let selector_function_with_nested_num = expr_utils::find_selector_function_exprs(exprs).len();

    // 1. There cannot be nested selection functions
    // 2. There cannot be multiple selection functions
    if selector_function_num == selector_function_with_nested_num {
        match selector_function_num {
            0 => return Ok(false),
            1 => return Ok(true),
            _ => {
                return Err(DataFusionError::Plan(format!(
                    "{}, found: {:#?}",
                    INVALID_EXPRS, exprs
                )))
            }
        }
    }

    Err(DataFusionError::Plan(format!(
        "{}, found: {:#?}",
        INVALID_EXPRS, exprs
    )))
}

fn extract_bottom_function(exprs: &[Expr]) -> Option<Expr> {
    expr_utils::find_exprs_in_exprs(exprs, &|nested_expr| {
        matches!(
            nested_expr,
            Expr::ScalarUDF {
                fun,
                ..
            } if fun.name.eq_ignore_ascii_case(BOTTOM)
        )
    })
    .first()
    .cloned()
}

fn extract_args(expr: &Expr) -> Result<(Expr, usize)> {
    if let Expr::ScalarUDF { fun: _, args } = expr {
        if args.len() != 2 {
            return Err(DataFusionError::Plan(INVALID_ARGUMENTS.to_string()));
        }

        let field_expr = args
            .get(0)
            .ok_or_else(|| DataFusionError::Plan(INVALID_ARGUMENTS.to_string()))?;
        let k_expr = args
            .get(1)
            .ok_or_else(|| DataFusionError::Plan(INVALID_ARGUMENTS.to_string()))?;

        let k = extract_args_k(k_expr)?;

        return Ok((field_expr.clone(), k));
    }

    Err(DataFusionError::Plan(INVALID_EXPRS.to_string()))
}

/// Extract the k value and check the value range
fn extract_args_k(expr: &Expr) -> Result<usize> {
    if let Expr::Literal(val) = expr.clone() {
        let k = match val {
            ScalarValue::UInt8(Some(v)) => v as usize,
            ScalarValue::UInt16(Some(v)) if v < 256 => v as usize,
            ScalarValue::UInt32(Some(v)) if v < 256 => v as usize,
            #[cfg(target_pointer_width = "64")]
            ScalarValue::UInt64(Some(v)) if v < 256 => v as usize,
            ScalarValue::Int8(Some(v)) if v > 0 => v as usize,
            ScalarValue::Int16(Some(v)) if v > 0 && v < 256 => v as usize,
            ScalarValue::Int32(Some(v)) if v > 0 && v < 256 => v as usize,
            #[cfg(target_pointer_width = "64")]
            ScalarValue::Int64(Some(v)) if v > 0 && v < 256 => v as usize,
            _ => return Err(DataFusionError::Plan(INVALID_ARGUMENTS.to_string())),
        };

        return Ok(k);
    }

    Err(DataFusionError::Plan(INVALID_ARGUMENTS.to_string()))
}
