#[cfg(test)]
#[path = "return_optimization_test.rs"]
mod test;

use cairo_lang_semantic as semantic;

use crate::borrow_check::analysis::{Analyzer, BackAnalysis, StatementLocation};
use crate::db::LoweringGroup;
use crate::{
    BlockId, FlatBlockEnd, FlatLowered, MatchEnumInfo, MatchInfo, Statement,
    StatementEnumConstruct, VarRemapping, VarUsage, VariableId,
};

/// Optimizes EnumConstruct statements where all the arms reconstruct the enum and return it.
pub fn return_optimization(db: &dyn LoweringGroup, lowered: &mut FlatLowered) {
    if !lowered.blocks.is_empty() {
        let ctx = ReturnOptimizerContext {
            lowered,
            unit_ty: semantic::corelib::unit_ty(db.upcast()),
            fixes: vec![],
        };
        let mut analysis =
            BackAnalysis { lowered: &*lowered, block_info: Default::default(), analyzer: ctx };
        analysis.get_root_info();
        let ctx = analysis.analyzer;

        for FixInfo { block_id, return_vars } in ctx.fixes.into_iter() {
            let block = &mut lowered.blocks[block_id];
            block.end = FlatBlockEnd::Return(return_vars)
        }
    }
}
#[allow(dead_code)]
pub struct ReturnOptimizerContext<'a> {
    lowered: &'a FlatLowered,
    unit_ty: semantic::TypeId,

    /// The list of fixes that should be applied.
    fixes: Vec<FixInfo>,
}

/// Information about a fix that should be applied to the lowering.
pub struct FixInfo {
    /// a block id of a block that can be fixed to return `return_vars`.
    block_id: BlockId,
    /// The variables that should be returned by the block.
    return_vars: Vec<VarUsage>,
}

/// The pattern that was detected in the backwards analysis.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pattern<'a> {
    /// No relevant pattern was found
    None,
    /// Found a return statement with the given returned
    Return(&'a [VarUsage]),

    /// Found an EnumConstruct whose output is returned by the function.
    EnumConstruct {
        /// The input to the EnumConstruct is either a unit type or `opt_input_var.unwrap()`.
        opt_input_var: Option<VariableId>,
        /// The constructed variant.
        variant: &'a semantic::ConcreteVariant,
        /// The returned variables of the return that follows the EnumConstruct.
        returned_vars: &'a [VarUsage],
    },
}

impl<'a> Analyzer<'a> for ReturnOptimizerContext<'_> {
    type Info = Pattern<'a>;

    fn visit_stmt(
        &mut self,
        info: &mut Self::Info,
        _statement_location: StatementLocation,
        stmt: &'a Statement,
    ) {
        match stmt {
            // Allow `StructConstruct` statements between the EnumMatch and the return statement.
            // This is ok since we check that that returned var is the result of the EnumConstruct.
            Statement::StructConstruct(_) => {}
            Statement::EnumConstruct(StatementEnumConstruct { variant, input, output }) => {
                if let Pattern::Return(returned_vars) = info {
                    let input_var = returned_vars.last().unwrap().var_id;
                    if &input_var != output {
                        *info = Pattern::None;
                        return;
                    }

                    // If the input is a unit type accept any variable in its place,
                    // otherwise save the expected input variable.
                    let input_requirement =
                        if self.lowered.variables[input.var_id].ty == self.unit_ty {
                            None
                        } else {
                            Some(input.var_id)
                        };

                    *info = Pattern::EnumConstruct {
                        opt_input_var: input_requirement,
                        variant,
                        returned_vars,
                    };
                }
            }
            _ => *info = Pattern::None,
        }
    }

    fn visit_goto(
        &mut self,
        info: &mut Self::Info,
        _statement_location: StatementLocation,
        _target_block_id: BlockId,
        remapping: &VarRemapping,
    ) {
        if !remapping.is_empty() {
            *info = Pattern::None;
        }
    }

    fn merge_match(
        &mut self,
        (block_id, _statement_idx): StatementLocation,
        match_info: &'a MatchInfo,
        infos: &[Self::Info],
    ) -> Self::Info {
        let MatchInfo::Enum(MatchEnumInfo { input, arms, .. }) = match_info else {
            return Pattern::None;
        };

        let [Pattern::EnumConstruct { returned_vars, .. }, ..] = infos else {
            return Pattern::None;
        };
        let mut return_vars = returned_vars.to_vec();
        return_vars.pop();

        for (arm, info) in arms.iter().zip(infos) {
            let Pattern::EnumConstruct { opt_input_var, variant, returned_vars } = info else {
                return Pattern::None;
            };

            if &&(arm.variant_id) != variant {
                return Pattern::None;
            }

            if let Some(var_id) = opt_input_var {
                if [*var_id] != arm.var_ids.as_slice() {
                    return Pattern::None;
                }
            }

            // If any of the returned vars is different we can't apply the optimization.
            if return_vars.len() + 1 != returned_vars.len()
                || return_vars
                    .iter()
                    .zip(returned_vars.iter())
                    .any(|(usage_a, uasge_b)| usage_a.var_id != uasge_b.var_id)
            {
                return Pattern::None;
            }
        }
        return_vars.push(*input);

        self.fixes.push(FixInfo { block_id, return_vars });
        Pattern::None
    }

    fn info_from_return(
        &mut self,
        _statement_location: StatementLocation,
        vars: &'a [VarUsage],
    ) -> Self::Info {
        Pattern::Return(vars)
    }

    fn info_from_panic(
        &mut self,
        _statement_location: StatementLocation,
        _data: &VarUsage,
    ) -> Self::Info {
        Pattern::None
    }
}