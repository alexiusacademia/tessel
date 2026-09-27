//! Encoders and decoders for `toJson`/`fromJson`/`toBinary`/`fromBinary`.
//!
//! For each type that is saved or loaded, a helper is generated:
//!
//! - `ptr tsl_h_enc_<T>(ptr value)` builds a tree (see the runtime's
//!   `codec.rs`) from the value at `value` (borrowed);
//! - `i8 tsl_h_dec_<T>(ptr tree, ptr out)` reads a tree into `out`; on
//!   failure it returns 0 and leaves nothing to release.
//!
//! Structs are objects keyed by field name: missing fields take their
//! defaults (or nil), unknown ones are ignored. Enum cases are their name, or
//! `{"case": {"label": value, …}}` when they carry values. Optionals are the
//! value or null, lists arrays, dictionaries objects (keys as text), ranges
//! `[start, end]`, Data a base64 string.

use inkwell::IntPredicate;
use inkwell::module::Linkage;
use inkwell::types::{BasicMetadataTypeEnum, BasicTypeEnum};
use inkwell::values::{FunctionValue, IntValue, PointerValue};

use super::{Gen, Val};
use crate::types::Ty;

impl<'ctx, 'a> Gen<'ctx, 'a> {
    fn tree_fn(&mut self, name: &str, params: &[&str], ret: Option<&str>) -> FunctionValue<'ctx> {
        let ty = |g: &Self, t: &str| -> BasicTypeEnum<'ctx> {
            match t {
                "i8" => g.i8().into(),
                "i32" => g.ctx.i32_type().into(),
                "i64" => g.i64().into(),
                "f64" => g.ctx.f64_type().into(),
                _ => g.ptr_ty().into(),
            }
        };
        let ps: Vec<BasicMetadataTypeEnum<'ctx>> = params.iter().map(|p| ty(self, p).into()).collect();
        let r = ret.map(|r| ty(self, r));
        self.declare_rt(name, &ps, r)
    }

    fn tree_call(&mut self, name: &str, params: &[&str], ret: Option<&str>, args: &[Val<'ctx>]) -> Option<Val<'ctx>> {
        let f = self.tree_fn(name, params, ret);
        let args: Vec<_> = args.iter().map(|a| (*a).into()).collect();
        self.b.build_call(f, &args, "").unwrap().try_as_basic_value().basic()
    }

    /// Emits `for i in 0..n { body(i) }`.
    pub(super) fn counted_loop(&mut self, n: IntValue<'ctx>, body: &mut dyn FnMut(&mut Self, IntValue<'ctx>)) {
        let i_slot = self.alloca(self.i64().into(), "i");
        self.b.build_store(i_slot, self.i64().const_zero()).unwrap();
        let (cond, work, done) = (self.block_named("loop"), self.block_named("step"), self.block_named("looped"));
        self.br(cond);
        self.b.position_at_end(cond);
        let i = self.b.build_load(self.i64(), i_slot, "").unwrap().into_int_value();
        let more = self.b.build_int_compare(IntPredicate::SLT, i, n, "").unwrap();
        self.b.build_conditional_branch(more, work, done).unwrap();
        self.b.position_at_end(work);
        body(self, i);
        let next = self.b.build_int_add(i, self.i64().const_int(1, false), "").unwrap();
        self.b.build_store(i_slot, next).unwrap();
        self.br(cond);
        self.b.position_at_end(done);
    }

    /// Runs `body` only while `ok` (an i1 slot) is still true.
    fn if_ok(&mut self, ok: PointerValue<'ctx>, body: &mut dyn FnMut(&mut Self)) {
        let flag = self.b.build_load(self.ctx.bool_type(), ok, "").unwrap().into_int_value();
        let (run, after) = (self.block_named("ok"), self.block_named("after"));
        self.b.build_conditional_branch(flag, run, after).unwrap();
        self.b.position_at_end(run);
        body(self);
        self.br(after);
        self.b.position_at_end(after);
    }

    fn i1(&self, b: bool) -> IntValue<'ctx> {
        self.ctx.bool_type().const_int(b as u64, false)
    }

    // ===== encoding =====

    pub(super) fn enc_helper(&mut self, t: &Ty) -> FunctionValue<'ctx> {
        let name = format!("tsl_h_enc_{}", self.key(t));
        if let Some(f) = self.helpers.get(&name) {
            return *f;
        }
        let p = self.ptr_ty();
        let f = self.module.add_function(&name, p.fn_type(&[p.into()], false), Some(Linkage::Internal));
        self.helpers.insert(name, f);
        let t = t.clone();
        self.in_helper(f, |g| {
            let vp = f.get_nth_param(0).unwrap().into_pointer_value();
            let tree = g.encode(&t, vp);
            g.b.build_return(Some(&tree)).unwrap();
        });
        f
    }

    /// Encodes the value at `vp` with its type's helper.
    fn encode_via(&mut self, t: &Ty, vp: PointerValue<'ctx>) -> PointerValue<'ctx> {
        let f = self.enc_helper(t);
        self.b.build_call(f, &[vp.into()], "").unwrap().try_as_basic_value().basic().unwrap().into_pointer_value()
    }

    fn encode(&mut self, t: &Ty, vp: PointerValue<'ctx>) -> PointerValue<'ctx> {
        let p = |v: Option<Val<'ctx>>| v.unwrap().into_pointer_value();
        match t {
            Ty::Int => {
                let v = self.load(t, vp);
                p(self.tree_call("tsl_tree_int", &["i64"], Some("ptr"), &[v]))
            }
            Ty::Float => {
                let v = self.load(t, vp);
                p(self.tree_call("tsl_tree_float", &["f64"], Some("ptr"), &[v]))
            }
            Ty::Bool => {
                let v = self.load(t, vp).into_int_value();
                let b = self.to_c_bool(v);
                p(self.tree_call("tsl_tree_bool", &["i8"], Some("ptr"), &[b]))
            }
            Ty::Str | Ty::Data => {
                let v = self.load(t, vp);
                let f = if *t == Ty::Str { "tsl_tree_str" } else { "tsl_tree_data" };
                p(self.tree_call(f, &["ptr"], Some("ptr"), &[v]))
            }
            Ty::Range => {
                let arr = p(self.tree_call("tsl_tree_array", &[], Some("ptr"), &[]));
                let v = self.load(t, vp).into_struct_value();
                for i in 0..2 {
                    let x = self.b.build_extract_value(v, i, "").unwrap();
                    let item = p(self.tree_call("tsl_tree_int", &["i64"], Some("ptr"), &[x]));
                    self.tree_call("tsl_tree_push", &["ptr", "ptr"], None, &[arr.into(), item.into()]);
                }
                arr
            }
            Ty::Opt(inner) => {
                let ot = self.llty(t).unwrap().into_struct_type();
                let slot = self.alloca(self.ptr_ty().into(), "tree");
                let fp = self.b.build_struct_gep(ot, vp, 0, "").unwrap();
                let flag = self.b.build_load(self.ctx.bool_type(), fp, "").unwrap().into_int_value();
                let (some, none, done) = (self.block_named("some"), self.block_named("none"), self.block_named("opt"));
                self.b.build_conditional_branch(flag, some, none).unwrap();
                self.b.position_at_end(some);
                let ip = self.b.build_struct_gep(ot, vp, 1, "").unwrap();
                let tree = self.encode_via(inner, ip);
                self.b.build_store(slot, tree).unwrap();
                self.br(done);
                self.b.position_at_end(none);
                let null = p(self.tree_call("tsl_tree_null", &[], Some("ptr"), &[]));
                self.b.build_store(slot, null).unwrap();
                self.br(done);
                self.b.position_at_end(done);
                self.b.build_load(self.ptr_ty(), slot, "").unwrap().into_pointer_value()
            }
            Ty::List(elem) => {
                let arr = p(self.tree_call("tsl_tree_array", &[], Some("ptr"), &[]));
                let list = self.load(t, vp);
                let n = self.call_rt("tsl_list_len", &[list]).unwrap().into_int_value();
                let null = self.ptr_ty().const_null();
                let elem = (**elem).clone();
                self.counted_loop(n, &mut |g, i| {
                    let ep = g.call_rt("tsl_list_get", &[list, i.into(), null.into()]).unwrap().into_pointer_value();
                    let item = g.encode_via(&elem, ep);
                    g.tree_call("tsl_tree_push", &["ptr", "ptr"], None, &[arr.into(), item.into()]);
                });
                arr
            }
            Ty::Dict(kt, vt) => {
                let obj = p(self.tree_call("tsl_tree_object", &[], Some("ptr"), &[]));
                let d = self.load(t, vp);
                let keys = self.call_rt("tsl_dict_keys", &[d]).unwrap();
                let values = self.call_rt("tsl_dict_values", &[d]).unwrap();
                let n = self.call_rt("tsl_list_len", &[keys]).unwrap().into_int_value();
                let null = self.ptr_ty().const_null();
                let (kt, vt) = ((**kt).clone(), (**vt).clone());
                self.counted_loop(n, &mut |g, i| {
                    let kp = g.call_rt("tsl_list_get", &[keys, i.into(), null.into()]).unwrap().into_pointer_value();
                    let k = g.load(&kt, kp);
                    // Keys as text: Strings as they are, others as they print.
                    let (text, owned) = if kt == Ty::Str { (k, false) } else { (g.stringify(k, &kt), kt != Ty::Str) };
                    let vp = g.call_rt("tsl_list_get", &[values, i.into(), null.into()]).unwrap().into_pointer_value();
                    let item = g.encode_via(&vt, vp);
                    g.tree_call("tsl_tree_set", &["ptr", "ptr", "ptr"], None, &[obj.into(), text, item.into()]);
                    if owned && matches!(kt, Ty::Int | Ty::Bool) {
                        g.release(text, &Ty::Str);
                    }
                });
                self.release(keys, &Ty::list(kt.clone()));
                self.release(values, &Ty::list(vt.clone()));
                obj
            }
            Ty::Struct(id) => {
                let obj = p(self.tree_call("tsl_tree_object", &[], Some("ptr"), &[]));
                let st = self.struct_ty(*id);
                let fields: Vec<(String, Ty)> =
                    self.checked.structs[*id].fields.iter().map(|f| (f.name.clone(), f.ty.clone())).collect();
                for (i, (name, ft)) in fields.iter().enumerate() {
                    let fp = self.b.build_struct_gep(st, vp, i as u32, "").unwrap();
                    let item = self.encode_via(ft, fp);
                    let key = self.str_lit(name);
                    self.tree_call("tsl_tree_set", &["ptr", "ptr", "ptr"], None, &[obj.into(), key.into(), item.into()]);
                }
                obj
            }
            Ty::Enum(id) => self.encode_enum(*id, vp),
            _ => unreachable!("the checker rejects saving {t:?}"),
        }
    }

    fn encode_enum(&mut self, id: usize, vp: PointerValue<'ctx>) -> PointerValue<'ctx> {
        let t = Ty::Enum(id);
        let payload = self.has_payload(id);
        let v = self.load(&t, vp);
        let (tag, bx) = if payload {
            let s = v.into_struct_value();
            let tag = self.b.build_extract_value(s, 0, "").unwrap().into_int_value();
            (tag, Some(self.b.build_extract_value(s, 1, "").unwrap().into_pointer_value()))
        } else {
            (v.into_int_value(), None)
        };
        let cases: Vec<(String, Vec<(String, Ty)>)> = self.checked.enums[id]
            .cases
            .iter()
            .map(|c| (c.name.clone(), c.fields.iter().map(|f| (f.label.clone(), f.ty.clone())).collect()))
            .collect();
        let slot = self.alloca(self.ptr_ty().into(), "tree");
        let done = self.block_named("encoded");
        let mut arms = Vec::new();
        for (i, (name, fields)) in cases.iter().enumerate() {
            let bb = self.block_named("case");
            arms.push((self.ctx.i32_type().const_int(i as u64, false), bb));
            let here = self.b.get_insert_block().unwrap();
            self.b.position_at_end(bb);
            let name_s = self.str_lit(name);
            let tree = if fields.is_empty() {
                self.tree_call("tsl_tree_str", &["ptr"], Some("ptr"), &[name_s.into()]).unwrap().into_pointer_value()
            } else {
                let inner = self.tree_call("tsl_tree_object", &[], Some("ptr"), &[]).unwrap().into_pointer_value();
                let pt = self.payload_ty(id, i);
                let data = self.box_data(bx.unwrap());
                for (j, (label, ft)) in fields.iter().enumerate() {
                    let fp = self.b.build_struct_gep(pt, data, j as u32, "").unwrap();
                    let item = self.encode_via(ft, fp);
                    let key = self.str_lit(label);
                    self.tree_call("tsl_tree_set", &["ptr", "ptr", "ptr"], None, &[inner.into(), key.into(), item.into()]);
                }
                let outer = self.tree_call("tsl_tree_object", &[], Some("ptr"), &[]).unwrap().into_pointer_value();
                self.tree_call("tsl_tree_set", &["ptr", "ptr", "ptr"], None, &[outer.into(), name_s.into(), inner.into()]);
                outer
            };
            self.b.build_store(slot, tree).unwrap();
            self.br(done);
            self.b.position_at_end(here);
        }
        self.b.build_switch(tag, done, &arms).unwrap();
        self.b.position_at_end(done);
        self.b.build_load(self.ptr_ty(), slot, "").unwrap().into_pointer_value()
    }

    // ===== decoding =====

    pub(super) fn dec_helper(&mut self, t: &Ty) -> FunctionValue<'ctx> {
        let name = format!("tsl_h_dec_{}", self.key(t));
        if let Some(f) = self.helpers.get(&name) {
            return *f;
        }
        let p = self.ptr_ty();
        let f = self.module.add_function(&name, self.i8().fn_type(&[p.into(), p.into()], false), Some(Linkage::Internal));
        self.helpers.insert(name, f);
        let t = t.clone();
        self.in_helper(f, |g| {
            let h = f.get_nth_param(0).unwrap().into_pointer_value();
            let out = f.get_nth_param(1).unwrap().into_pointer_value();
            let ok = g.decode(&t, h, out);
            let r = g.to_c_bool(ok);
            g.b.build_return(Some(&r)).unwrap();
        });
        f
    }

    /// Decodes tree `h` into `out` with its type's helper; true if it worked.
    fn decode_via(&mut self, t: &Ty, h: PointerValue<'ctx>, out: PointerValue<'ctx>) -> IntValue<'ctx> {
        let f = self.dec_helper(t);
        let r = self.b.build_call(f, &[h.into(), out.into()], "").unwrap().try_as_basic_value().basic();
        self.truthy(r)
    }

    fn tree_kind(&mut self, h: PointerValue<'ctx>) -> IntValue<'ctx> {
        self.tree_call("tsl_tree_kind", &["ptr"], Some("i32"), &[h.into()]).unwrap().into_int_value()
    }

    fn kind_is(&mut self, h: PointerValue<'ctx>, kind: u64) -> IntValue<'ctx> {
        let k = self.tree_kind(h);
        self.b.build_int_compare(IntPredicate::EQ, k, self.ctx.i32_type().const_int(kind, false), "").unwrap()
    }

    fn decode(&mut self, t: &Ty, h: PointerValue<'ctx>, out: PointerValue<'ctx>) -> IntValue<'ctx> {
        match t {
            Ty::Int => {
                let r = self.tree_call("tsl_tree_as_int", &["ptr", "ptr"], Some("i8"), &[h.into(), out.into()]);
                self.truthy(r)
            }
            Ty::Float => {
                let r = self.tree_call("tsl_tree_as_float", &["ptr", "ptr"], Some("i8"), &[h.into(), out.into()]);
                self.truthy(r)
            }
            Ty::Bool => {
                let tmp = self.alloca(self.i8().into(), "b");
                let r = self.tree_call("tsl_tree_as_bool", &["ptr", "ptr"], Some("i8"), &[h.into(), tmp.into()]);
                let byte = self.b.build_load(self.i8(), tmp, "").unwrap().into_int_value();
                let b = self.b.build_int_compare(IntPredicate::NE, byte, self.i8().const_zero(), "").unwrap();
                self.b.build_store(out, b).unwrap();
                self.truthy(r)
            }
            Ty::Str | Ty::Data => {
                let f = if *t == Ty::Str { "tsl_tree_as_str" } else { "tsl_tree_as_data" };
                let s = self.tree_call(f, &["ptr"], Some("ptr"), &[h.into()]).unwrap().into_pointer_value();
                self.b.build_store(out, s).unwrap();
                self.b.build_is_not_null(s, "").unwrap()
            }
            Ty::Range => {
                let ok = self.alloca(self.ctx.bool_type().into(), "ok");
                let is_arr = self.kind_is(h, 4);
                let n = self.tree_call("tsl_tree_len", &["ptr"], Some("i64"), &[h.into()]).unwrap().into_int_value();
                let two = self.b.build_int_compare(IntPredicate::EQ, n, self.i64().const_int(2, false), "").unwrap();
                let fine = self.b.build_and(is_arr, two, "").unwrap();
                self.b.build_store(ok, fine).unwrap();
                let rt = self.llty(t).unwrap().into_struct_type();
                for i in 0..2u32 {
                    self.if_ok(ok, &mut |g| {
                        let item = g.tree_call("tsl_tree_at", &["ptr", "i64"], Some("ptr"), &[h.into(), g.i64().const_int(i as u64, false).into()]);
                        let fp = g.b.build_struct_gep(rt, out, i, "").unwrap();
                        let got = g.decode_via(&Ty::Int, item.unwrap().into_pointer_value(), fp);
                        g.b.build_store(ok, got).unwrap();
                    });
                }
                self.b.build_load(self.ctx.bool_type(), ok, "").unwrap().into_int_value()
            }
            Ty::Opt(inner) => {
                let ok = self.alloca(self.ctx.bool_type().into(), "ok");
                let is_null = self.kind_is(h, 0);
                let (none, some, done) = (self.block_named("null"), self.block_named("value"), self.block_named("opt"));
                self.b.build_conditional_branch(is_null, none, some).unwrap();
                self.b.position_at_end(none);
                let nv = self.none(t);
                self.b.build_store(out, nv).unwrap();
                self.b.build_store(ok, self.i1(true)).unwrap();
                self.br(done);
                self.b.position_at_end(some);
                let il = self.llty(inner).unwrap();
                let tmp = self.alloca(il, "inner");
                let got = self.decode_via(inner, h, tmp);
                self.b.build_store(ok, got).unwrap();
                self.if_ok(ok, &mut |g| {
                    let v = g.load(inner, tmp);
                    let s = g.some(t, v);
                    g.b.build_store(out, s).unwrap();
                });
                self.br(done);
                self.b.position_at_end(done);
                self.b.build_load(self.ctx.bool_type(), ok, "").unwrap().into_int_value()
            }
            Ty::List(elem) => {
                let elem = (**elem).clone();
                let ok = self.alloca(self.ctx.bool_type().into(), "ok");
                let is_arr = self.kind_is(h, 4);
                self.b.build_store(ok, is_arr).unwrap();
                let list_slot = self.alloca(self.ptr_ty().into(), "list");
                self.b.build_store(list_slot, self.ptr_ty().const_null()).unwrap();
                self.if_ok(ok, &mut |g| {
                    let n = g.tree_call("tsl_tree_len", &["ptr"], Some("i64"), &[h.into()]).unwrap().into_int_value();
                    let info = g.type_info(&elem);
                    let list = g.call_rt("tsl_list_new", &[info.into(), n.into()]).unwrap();
                    g.b.build_store(list_slot, list).unwrap();
                    let el = g.llty(&elem).unwrap();
                    let tmp = g.alloca(el, "item");
                    g.counted_loop(n, &mut |g, i| {
                        g.if_ok(ok, &mut |g| {
                            let item = g.tree_call("tsl_tree_at", &["ptr", "i64"], Some("ptr"), &[h.into(), i.into()]).unwrap();
                            let got = g.decode_via(&elem, item.into_pointer_value(), tmp);
                            g.b.build_store(ok, got).unwrap();
                            g.if_ok(ok, &mut |g| {
                                g.call_rt("tsl_list_push", &[list_slot.into(), tmp.into()]);
                            });
                        });
                    });
                });
                let fine = self.b.build_load(self.ctx.bool_type(), ok, "").unwrap().into_int_value();
                let list = self.b.build_load(self.ptr_ty(), list_slot, "").unwrap();
                let (good, bad, done) = (self.block_named("good"), self.block_named("bad"), self.block_named("list"));
                self.b.build_conditional_branch(fine, good, bad).unwrap();
                self.b.position_at_end(good);
                self.b.build_store(out, list).unwrap();
                self.br(done);
                self.b.position_at_end(bad);
                // (A null list — not an array — is safe to release.)
                self.call_rt("tsl_release", &[list]);
                self.br(done);
                self.b.position_at_end(done);
                fine
            }
            Ty::Dict(kt, vt) => self.decode_dict(t, kt, vt, h, out),
            Ty::Struct(id) => self.decode_struct(*id, h, out),
            Ty::Enum(id) => self.decode_enum(*id, h, out),
            _ => unreachable!("the checker rejects loading {t:?}"),
        }
    }

    fn decode_dict(&mut self, t: &Ty, kt: &Ty, vt: &Ty, h: PointerValue<'ctx>, out: PointerValue<'ctx>) -> IntValue<'ctx> {
        let (kt, vt) = (kt.clone(), vt.clone());
        let ok = self.alloca(self.ctx.bool_type().into(), "ok");
        let is_obj = self.kind_is(h, 5);
        self.b.build_store(ok, is_obj).unwrap();
        let (ki, vi) = (self.type_info(&kt), self.type_info(&vt));
        let d = self.call_rt("tsl_dict_new", &[ki.into(), vi.into()]).unwrap();
        let slot = self.spill(t, d);
        self.if_ok(ok, &mut |g| {
            let n = g.tree_call("tsl_tree_len", &["ptr"], Some("i64"), &[h.into()]).unwrap().into_int_value();
            let kl = g.llty(&kt).unwrap();
            let vl = g.llty(&vt).unwrap();
            let (kp, vp) = (g.alloca(kl, "key"), g.alloca(vl, "value"));
            g.counted_loop(n, &mut |g, i| {
                g.if_ok(ok, &mut |g| {
                    let text = g.tree_call("tsl_tree_key_at", &["ptr", "i64"], Some("ptr"), &[h.into(), i.into()]).unwrap();
                    // The key, from its text.
                    let key_ok = match &kt {
                        Ty::Str => {
                            g.b.build_store(kp, text).unwrap();
                            g.i1(true)
                        }
                        Ty::Int => {
                            let r = g.call_rt_generic("tsl_str_to_int", &[(text, Ty::Str)], &Ty::opt(Ty::Int)).unwrap();
                            let s = r.into_struct_value();
                            let present = g.b.build_extract_value(s, 0, "").unwrap().into_int_value();
                            let v = g.b.build_extract_value(s, 1, "").unwrap();
                            g.b.build_store(kp, v).unwrap();
                            g.release(text, &Ty::Str);
                            present
                        }
                        Ty::Bool => {
                            let t_lit = g.str_lit("true");
                            let is_true = g.call_rt("tsl_str_eq", &[text, t_lit.into()]);
                            let is_true = g.truthy(is_true);
                            let f_lit = g.str_lit("false");
                            let is_false = g.call_rt("tsl_str_eq", &[text, f_lit.into()]);
                            let is_false = g.truthy(is_false);
                            g.b.build_store(kp, is_true).unwrap();
                            g.release(text, &Ty::Str);
                            g.b.build_or(is_true, is_false, "").unwrap()
                        }
                        Ty::Enum(id) => {
                            let tree = g.tree_call("tsl_tree_str", &["ptr"], Some("ptr"), &[text]).unwrap().into_pointer_value();
                            let got = g.decode_via(&Ty::Enum(*id), tree, kp);
                            g.tree_call("tsl_tree_free", &["ptr"], None, &[tree.into()]);
                            g.release(text, &Ty::Str);
                            got
                        }
                        _ => unreachable!("the checker allows only these key types"),
                    };
                    g.b.build_store(ok, key_ok).unwrap();
                    g.if_ok(ok, &mut |g| {
                        let item = g.tree_call("tsl_tree_at", &["ptr", "i64"], Some("ptr"), &[h.into(), i.into()]).unwrap();
                        let got = g.decode_via(&vt, item.into_pointer_value(), vp);
                        g.b.build_store(ok, got).unwrap();
                        let (set, drop_key, next) = (g.block_named("set"), g.block_named("dropkey"), g.block_named("next"));
                        g.b.build_conditional_branch(got, set, drop_key).unwrap();
                        g.b.position_at_end(set);
                        g.call_rt("tsl_dict_set", &[slot.into(), kp.into(), vp.into()]);
                        g.br(next);
                        g.b.position_at_end(drop_key);
                        let k = g.load(&kt, kp);
                        g.release(k, &kt);
                        g.br(next);
                        g.b.position_at_end(next);
                    });
                });
            });
        });
        let fine = self.b.build_load(self.ctx.bool_type(), ok, "").unwrap().into_int_value();
        let d = self.load(t, slot);
        let (good, bad, done) = (self.block_named("good"), self.block_named("bad"), self.block_named("dict"));
        self.b.build_conditional_branch(fine, good, bad).unwrap();
        self.b.position_at_end(good);
        self.b.build_store(out, d).unwrap();
        self.br(done);
        self.b.position_at_end(bad);
        self.release(d, t);
        self.br(done);
        self.b.position_at_end(done);
        fine
    }

    /// Decodes named values (struct fields, enum case values) from object `h`
    /// into temporaries. On success returns them; on failure releases the
    /// ones already decoded. `defaults[i]` is the field's default (if any).
    fn decode_fields(
        &mut self,
        h: PointerValue<'ctx>,
        fields: &[(String, Ty)],
        default: &mut dyn FnMut(&mut Self, usize) -> Option<Val<'ctx>>,
    ) -> (IntValue<'ctx>, Vec<PointerValue<'ctx>>) {
        let ok = self.alloca(self.ctx.bool_type().into(), "ok");
        let is_obj = self.kind_is(h, 5);
        self.b.build_store(ok, is_obj).unwrap();
        let mut temps = Vec::new();
        let mut filled = Vec::new();
        for (name, ft) in fields {
            let lt = self.llty(ft).unwrap();
            temps.push(self.alloca(lt, name));
            let f = self.alloca(self.ctx.bool_type().into(), "filled");
            self.b.build_store(f, self.i1(false)).unwrap();
            filled.push(f);
        }
        for (i, (name, ft)) in fields.iter().enumerate() {
            let (tmp, fill) = (temps[i], filled[i]);
            self.if_ok(ok, &mut |g| {
                let key = g.str_lit(name);
                let child = g.tree_call("tsl_tree_field", &["ptr", "ptr"], Some("ptr"), &[h.into(), key.into()]).unwrap().into_pointer_value();
                let present = g.b.build_is_not_null(child, "").unwrap();
                let (found, missing, next) = (g.block_named("found"), g.block_named("missing"), g.block_named("field"));
                g.b.build_conditional_branch(present, found, missing).unwrap();
                g.b.position_at_end(found);
                let got = g.decode_via(ft, child, tmp);
                g.b.build_store(ok, got).unwrap();
                g.b.build_store(fill, got).unwrap();
                g.br(next);
                g.b.position_at_end(missing);
                // Missing: the default, or nil for an optional; otherwise it fails.
                match default(g, i) {
                    Some(v) => {
                        g.b.build_store(tmp, v).unwrap();
                        g.b.build_store(fill, g.i1(true)).unwrap();
                    }
                    None if matches!(ft, Ty::Opt(_)) => {
                        let nv = g.none(ft);
                        g.b.build_store(tmp, nv).unwrap();
                        g.b.build_store(fill, g.i1(true)).unwrap();
                    }
                    None => {
                        g.b.build_store(ok, g.i1(false)).unwrap();
                    }
                }
                g.br(next);
                g.b.position_at_end(next);
            });
        }
        let fine = self.b.build_load(self.ctx.bool_type(), ok, "").unwrap().into_int_value();
        let (bad, done) = (self.block_named("cleanup"), self.block_named("fields"));
        self.b.build_conditional_branch(fine, done, bad).unwrap();
        self.b.position_at_end(bad);
        for (i, (_, ft)) in fields.iter().enumerate() {
            if self.needs_rc(ft) {
                let (tmp, fill) = (temps[i], filled[i]);
                self.if_ok(fill, &mut |g| {
                    let v = g.load(ft, tmp);
                    g.release(v, ft);
                });
            }
        }
        self.br(done);
        self.b.position_at_end(done);
        (fine, temps)
    }

    fn decode_struct(&mut self, id: usize, h: PointerValue<'ctx>, out: PointerValue<'ctx>) -> IntValue<'ctx> {
        let fields: Vec<(String, Ty)> = self.checked.structs[id].fields.iter().map(|f| (f.name.clone(), f.ty.clone())).collect();
        let decl = self.struct_decls.get(&id).copied();
        let (ok, temps) = self.decode_fields(h, &fields, &mut |g, i| {
            let default = decl?.fields[i].default.as_ref()?;
            let ft = g.checked.structs[id].fields[i].ty.clone();
            g.expr_as(default, &ft)
        });
        let st = self.struct_ty(id);
        let ok_slot = self.alloca(self.ctx.bool_type().into(), "ok");
        self.b.build_store(ok_slot, ok).unwrap();
        self.if_ok(ok_slot, &mut |g| {
            for (i, (_, ft)) in fields.iter().enumerate() {
                let v = g.load(ft, temps[i]);
                let fp = g.b.build_struct_gep(st, out, i as u32, "").unwrap();
                g.b.build_store(fp, v).unwrap();
            }
        });
        ok
    }

    fn decode_enum(&mut self, id: usize, h: PointerValue<'ctx>, out: PointerValue<'ctx>) -> IntValue<'ctx> {
        let t = Ty::Enum(id);
        let payload = self.has_payload(id);
        let cases: Vec<(String, Vec<(String, Ty)>)> = self.checked.enums[id]
            .cases
            .iter()
            .map(|c| (c.name.clone(), c.fields.iter().map(|f| (f.label.clone(), f.ty.clone())).collect()))
            .collect();
        let ok = self.alloca(self.ctx.bool_type().into(), "ok");
        self.b.build_store(ok, self.i1(false)).unwrap();
        // The case name: a string, or the one key of an object.
        let name_slot = self.alloca(self.ptr_ty().into(), "name");
        let body_slot = self.alloca(self.ptr_ty().into(), "body");
        self.b.build_store(body_slot, self.ptr_ty().const_null()).unwrap();
        let is_str = self.kind_is(h, 3);
        let (as_str, as_obj, named) = (self.block_named("str"), self.block_named("obj"), self.block_named("named"));
        self.b.build_conditional_branch(is_str, as_str, as_obj).unwrap();
        self.b.position_at_end(as_str);
        let s = self.tree_call("tsl_tree_as_str", &["ptr"], Some("ptr"), &[h.into()]).unwrap();
        self.b.build_store(name_slot, s).unwrap();
        self.br(named);
        self.b.position_at_end(as_obj);
        let n = self.tree_call("tsl_tree_len", &["ptr"], Some("i64"), &[h.into()]).unwrap().into_int_value();
        let one = self.b.build_int_compare(IntPredicate::EQ, n, self.i64().const_int(1, false), "").unwrap();
        let zero = self.i64().const_zero();
        let (single, other) = (self.block_named("single"), self.block_named("other"));
        self.b.build_conditional_branch(one, single, other).unwrap();
        self.b.position_at_end(single);
        let key = self.tree_call("tsl_tree_key_at", &["ptr", "i64"], Some("ptr"), &[h.into(), zero.into()]).unwrap();
        self.b.build_store(name_slot, key).unwrap();
        let body = self.tree_call("tsl_tree_at", &["ptr", "i64"], Some("ptr"), &[h.into(), zero.into()]).unwrap();
        self.b.build_store(body_slot, body).unwrap();
        self.br(named);
        self.b.position_at_end(other);
        self.b.build_store(name_slot, self.ptr_ty().const_null()).unwrap();
        self.br(named);
        self.b.position_at_end(named);
        let name = self.b.build_load(self.ptr_ty(), name_slot, "").unwrap().into_pointer_value();
        let have_name = self.b.build_is_not_null(name, "").unwrap();
        let (check, finish) = (self.block_named("cases"), self.block_named("enum"));
        self.b.build_conditional_branch(have_name, check, finish).unwrap();
        self.b.position_at_end(check);
        for (i, (case_name, fields)) in cases.iter().enumerate() {
            let lit = self.str_lit(case_name);
            let same = self.call_rt("tsl_str_eq", &[name.into(), lit.into()]);
            let same = self.truthy(same);
            let (matched, next) = (self.block_named("match"), self.block_named("next"));
            self.b.build_conditional_branch(same, matched, next).unwrap();
            self.b.position_at_end(matched);
            let tag = self.ctx.i32_type().const_int(i as u64, false);
            if fields.is_empty() {
                let v: Val = if payload {
                    let st = self.llty(&t).unwrap().into_struct_type();
                    let s = self.b.build_insert_value(st.get_undef(), tag, 0, "").unwrap();
                    self.b.build_insert_value(s, self.ptr_ty().const_null(), 1, "").unwrap().into_struct_value().into()
                } else {
                    tag.into()
                };
                self.b.build_store(out, v).unwrap();
                self.b.build_store(ok, self.i1(true)).unwrap();
            } else {
                let body = self.b.build_load(self.ptr_ty(), body_slot, "").unwrap().into_pointer_value();
                let has_body = self.b.build_is_not_null(body, "").unwrap();
                let (decode_bb, skip) = (self.block_named("payload"), self.block_named("nopayload"));
                self.b.build_conditional_branch(has_body, decode_bb, skip).unwrap();
                self.b.position_at_end(decode_bb);
                let (got, temps) = self.decode_fields(body, fields, &mut |_, _| None);
                let got_slot = self.alloca(self.ctx.bool_type().into(), "got");
                self.b.build_store(got_slot, got).unwrap();
                self.if_ok(got_slot, &mut |g| {
                    let info = g.payload_info(id, i);
                    let bx = g.call_rt("tsl_box_new", &[info.into()]).unwrap().into_pointer_value();
                    let data = g.box_data(bx);
                    let pt = g.payload_ty(id, i);
                    for (j, (_, ft)) in fields.iter().enumerate() {
                        let v = g.load(ft, temps[j]);
                        let fp = g.b.build_struct_gep(pt, data, j as u32, "").unwrap();
                        g.b.build_store(fp, v).unwrap();
                    }
                    let st = g.llty(&t).unwrap().into_struct_type();
                    let s = g.b.build_insert_value(st.get_undef(), tag, 0, "").unwrap();
                    let s = g.b.build_insert_value(s, bx, 1, "").unwrap().into_struct_value();
                    g.b.build_store(out, s).unwrap();
                    g.b.build_store(ok, g.i1(true)).unwrap();
                });
                self.br(skip);
                self.b.position_at_end(skip);
            }
            self.br(finish);
            self.b.position_at_end(next);
        }
        self.br(finish);
        self.b.position_at_end(finish);
        let name = self.b.build_load(self.ptr_ty(), name_slot, "").unwrap();
        self.call_rt("tsl_release", &[name]);
        self.b.build_load(self.ctx.bool_type(), ok, "").unwrap().into_int_value()
    }
}
