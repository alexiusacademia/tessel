//! Generic list methods of the standard library: searching, counting,
//! sums and extremes, slices, and random picks. They work for any element
//! type, so they're generated here rather than being runtime calls.

use inkwell::values::IntValue;
use inkwell::IntPredicate;

use super::{Gen, Val};
use crate::ast::BinOp;
use crate::source::Span;
use crate::types::Ty;

impl<'ctx, 'a> Gen<'ctx, 'a> {
    /// Item `i` of `list`, as a borrowed value.
    fn item(&mut self, list: Val<'ctx>, elem: &Ty, i: IntValue<'ctx>) -> Val<'ctx> {
        let null = self.ptr_ty().const_null();
        let p = self.call_rt("tsl_list_get", &[list, i.into(), null.into()]).unwrap().into_pointer_value();
        self.load(elem, p)
    }

    fn opt_int(&mut self, slot: inkwell::values::PointerValue<'ctx>) -> Val<'ctx> {
        // -1 means none.
        let v = self.b.build_load(self.i64(), slot, "").unwrap().into_int_value();
        let present = self.b.build_int_compare(IntPredicate::SGE, v, self.i64().const_zero(), "").unwrap();
        let t = Ty::opt(Ty::Int);
        let st = self.llty(&t).unwrap().into_struct_type();
        let s = self.b.build_insert_value(st.get_undef(), present, 0, "").unwrap();
        self.b.build_insert_value(s, v, 1, "").unwrap().into_struct_value().into()
    }

    /// Calls `pred` (a `fn(T) -> Bool`) on a borrowed item.
    fn test(&mut self, pred: Val<'ctx>, pred_t: &Ty, item: Val<'ctx>, elem: &Ty) -> IntValue<'ctx> {
        self.retain(item, elem);
        self.call_fn_value(pred, pred_t, &[item]).unwrap().into_int_value()
    }

    /// Non-changing list methods added by the standard library, or None if
    /// `name` isn't one of them. `args` are the evaluated arguments (owned).
    pub(super) fn list_method(&mut self, list: Val<'ctx>, elem: &Ty, name: &str, args: &[Val<'ctx>], span: Span) -> Option<Val<'ctx>> {
        let len = self.call_rt("tsl_list_len", &[list]).unwrap().into_int_value();
        let pred_t = Ty::func(vec![elem.clone()], Ty::Bool);
        Some(match name {
            "index" | "lastIndex" | "firstIndex" => {
                let slot = self.alloca(self.i64().into(), "index");
                self.b.build_store(slot, self.i64().const_int(u64::MAX, true)).unwrap();
                let last = name == "lastIndex";
                self.counted_loop(len, &mut |g, i| {
                    let item = g.item(list, elem, i);
                    let hit = if name == "firstIndex" { g.test(args[0], &pred_t, item, elem) } else { g.eq(item, args[0], elem) };
                    // The first match (or, for lastIndex, the latest).
                    let current = g.b.build_load(g.i64(), slot, "").unwrap().into_int_value();
                    let unset = g.b.build_int_compare(IntPredicate::SLT, current, g.i64().const_zero(), "").unwrap();
                    let take = if last { hit } else { g.b.build_and(hit, unset, "").unwrap() };
                    let v = g.b.build_select(take, i, current, "").unwrap();
                    g.b.build_store(slot, v).unwrap();
                });
                if name == "firstIndex" {
                    self.release(args[0], &pred_t);
                } else {
                    self.release(args[0], elem);
                }
                self.opt_int(slot)
            }
            "count" | "any" | "all" => {
                let slot = self.alloca(self.i64().into(), "count");
                self.b.build_store(slot, self.i64().const_zero()).unwrap();
                self.counted_loop(len, &mut |g, i| {
                    let item = g.item(list, elem, i);
                    let hit = g.test(args[0], &pred_t, item, elem);
                    let add = g.b.build_int_z_extend(hit, g.i64(), "").unwrap();
                    let n = g.b.build_load(g.i64(), slot, "").unwrap().into_int_value();
                    let n = g.b.build_int_add(n, add, "").unwrap();
                    g.b.build_store(slot, n).unwrap();
                });
                self.release(args[0], &pred_t);
                let n = self.b.build_load(self.i64(), slot, "").unwrap().into_int_value();
                match name {
                    "count" => n.into(),
                    "any" => self.b.build_int_compare(IntPredicate::SGT, n, self.i64().const_zero(), "").unwrap().into(),
                    _ => self.b.build_int_compare(IntPredicate::EQ, n, len, "").unwrap().into(),
                }
            }
            "sum" => {
                let lt = self.llty(elem).unwrap();
                let slot = self.alloca(lt, "sum");
                self.b.build_store(slot, lt.const_zero()).unwrap();
                self.counted_loop(len, &mut |g, i| {
                    let item = g.item(list, elem, i);
                    let total = g.load(elem, slot);
                    let r = g.arith(BinOp::Add, total, item, elem, span);
                    g.b.build_store(slot, r).unwrap();
                });
                self.load(elem, slot)
            }
            "min" | "max" => {
                let opt = Ty::opt(elem.clone());
                let lt = self.llty(&opt).unwrap();
                let slot = self.alloca(lt, "best");
                let none = self.none(&opt);
                self.b.build_store(slot, none).unwrap();
                let op = if name == "min" { BinOp::Lt } else { BinOp::Gt };
                self.counted_loop(len, &mut |g, i| {
                    let item = g.item(list, elem, i);
                    let best = g.load(&opt, slot).into_struct_value();
                    let have = g.b.build_extract_value(best, 0, "").unwrap().into_int_value();
                    let (check, yes, done) = (g.block_named("check"), g.block_named("better"), g.block_named("kept"));
                    // Compare only once there is a best value (it may be a string).
                    g.b.build_conditional_branch(have, check, yes).unwrap();
                    g.b.position_at_end(check);
                    let current = g.b.build_extract_value(best, 1, "").unwrap();
                    let better = g.compare(op, item, current, elem);
                    g.b.build_conditional_branch(better, yes, done).unwrap();
                    g.b.position_at_end(yes);
                    g.retain(item, elem);
                    let old = g.load(&opt, slot);
                    g.release(old, &opt);
                    let s = g.some(&opt, item);
                    g.b.build_store(slot, s).unwrap();
                    g.br(done);
                    g.b.position_at_end(done);
                });
                self.load(&opt, slot)
            }
            "prefix" | "suffix" | "dropFirst" | "dropLast" | "slice" => {
                let zero = self.i64().const_zero();
                let one = self.i64().const_int(1, false);
                let n = args.first().map_or(one, |a| a.into_int_value());
                let (from, to) = match name {
                    "prefix" => (zero, n),
                    "suffix" => (self.b.build_int_sub(len, n, "").unwrap(), len),
                    "dropFirst" => (n, len),
                    "dropLast" => (zero, self.b.build_int_sub(len, n, "").unwrap()),
                    _ => (args[0].into_int_value(), args[1].into_int_value()),
                };
                let f = self.declare_rt(
                    "tsl_list_slice",
                    &[self.ptr_ty().into(), self.i64().into(), self.i64().into()],
                    Some(self.ptr_ty().into()),
                );
                self.b.build_call(f, &[list.into(), from.into(), to.into()], "").unwrap().try_as_basic_value().basic().unwrap()
            }
            "shuffled" => {
                let f = self.declare_rt("tsl_list_shuffled", &[self.ptr_ty().into()], Some(self.ptr_ty().into()));
                self.b.build_call(f, &[list.into()], "").unwrap().try_as_basic_value().basic().unwrap()
            }
            "unique" => {
                let info = self.type_info(elem);
                let zero = self.i64().const_zero();
                let out = self.call_rt("tsl_list_new", &[info.into(), zero.into()]).unwrap();
                let out_slot = self.spill(&Ty::list(elem.clone()), out);
                self.counted_loop(len, &mut |g, i| {
                    let item = g.item(list, elem, i);
                    let p = g.spill(elem, item);
                    let current = g.b.build_load(g.ptr_ty(), out_slot, "").unwrap();
                    let seen = g.call_rt("tsl_list_contains", &[current, p.into()]);
                    let seen = g.truthy(seen);
                    let (add, next) = (g.block_named("new"), g.block_named("seen"));
                    g.b.build_conditional_branch(seen, next, add).unwrap();
                    g.b.position_at_end(add);
                    g.retain(item, elem);
                    g.call_rt("tsl_list_push", &[out_slot.into(), p.into()]);
                    g.br(next);
                    g.b.position_at_end(next);
                });
                self.b.build_load(self.ptr_ty(), out_slot, "").unwrap()
            }
            "randomElement" => {
                let opt = Ty::opt(elem.clone());
                let lt = self.llty(&opt).unwrap();
                let slot = self.alloca(lt, "picked");
                let none = self.none(&opt);
                self.b.build_store(slot, none).unwrap();
                let empty = self.b.build_int_compare(IntPredicate::EQ, len, self.i64().const_zero(), "").unwrap();
                let (pick, done) = (self.block_named("pick"), self.block_named("picked"));
                self.b.build_conditional_branch(empty, done, pick).unwrap();
                self.b.position_at_end(pick);
                let f = self.declare_rt("tsl_random_int", &[self.i64().into(), self.i64().into()], Some(self.i64().into()));
                let zero = self.i64().const_zero();
                let i = self.b.build_call(f, &[zero.into(), len.into()], "").unwrap().try_as_basic_value().basic().unwrap();
                let item = self.item(list, elem, i.into_int_value());
                self.retain(item, elem);
                let s = self.some(&opt, item);
                self.b.build_store(slot, s).unwrap();
                self.br(done);
                self.b.position_at_end(done);
                self.load(&opt, slot)
            }
            _ => return None,
        })
    }

    /// `list.indices`: 0..count.
    pub(super) fn list_indices(&mut self, list: Val<'ctx>) -> Val<'ctx> {
        let len = self.call_rt("tsl_list_len", &[list]).unwrap();
        let st = self.llty(&Ty::Range).unwrap().into_struct_type();
        let s = self.b.build_insert_value(st.get_undef(), self.i64().const_zero(), 0, "").unwrap();
        self.b.build_insert_value(s, len, 1, "").unwrap().into_struct_value().into()
    }

    /// Changing list methods added by the standard library, on the list's
    /// storage `slot`. `args` are the evaluated arguments (owned).
    pub(super) fn list_mutating(
        &mut self,
        slot: inkwell::values::PointerValue<'ctx>,
        elem: &Ty,
        name: &str,
        args: &[Val<'ctx>],
        loc: inkwell::values::PointerValue<'ctx>,
    ) -> Option<Option<Val<'ctx>>> {
        Some(match name {
            "removeFirst" | "removeLast" => {
                let list = self.b.build_load(self.ptr_ty(), slot, "").unwrap();
                let len = self.call_rt("tsl_list_len", &[list]).unwrap().into_int_value();
                let i = if name == "removeFirst" {
                    self.i64().const_zero()
                } else {
                    self.b.build_int_sub(len, self.i64().const_int(1, false), "").unwrap()
                };
                let lt = self.llty(elem).unwrap();
                let out = self.alloca(lt, "removed");
                self.call_rt("tsl_list_remove", &[slot.into(), i.into(), out.into(), loc.into()]);
                Some(self.load(elem, out))
            }
            "appendAll" => {
                let other = args[0];
                let len = self.call_rt("tsl_list_len", &[other]).unwrap().into_int_value();
                self.counted_loop(len, &mut |g, i| {
                    let item = g.item(other, elem, i);
                    g.retain(item, elem);
                    let p = g.spill(elem, item);
                    g.call_rt("tsl_list_push", &[slot.into(), p.into()]);
                });
                self.release(other, &Ty::list(elem.clone()));
                None
            }
            _ => return None,
        })
    }
}
