# SSA Construction Pass

## Overview

`karte-lir/src/pass/ssa_construction.rs` — converts LIR into SSA form by inserting Phi nodes and renaming register definitions/uses.

## Critical: Dominator Tree Traversal

`rename_block_recursive` MUST use dominator tree children (from `immediate_dominators` table), NOT CFG successor edges + idom checks. CFG successor traversal misses merge blocks whose idom is an earlier (non-successor) block.

## Pass Interaction

SSA runs before: Memory2Reg → Phi Elimination → Register Allocation. If SSA misses blocks, downstream passes see stale register references.
