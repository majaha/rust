use smallvec::SmallVec;
use std::fmt::Debug;
use std::hash::Hash;

use crate::fold::TypeSuperFoldable;
use crate::visit::{Flags, TypeSuperVisitable, TypeVisitable};
use crate::{
    new, BoundVar, BoundVars, CanonicalVarInfo, ConstKind, DebugWithInfcx, RegionKind, TyKind,
    UniverseIndex,
};

pub trait Interner: Sized + Copy {
    type DefId: Copy + Debug + Hash + Eq;
    type AdtDef: Copy + Debug + Hash + Eq;

    type GenericArgs: Copy
        + DebugWithInfcx<Self>
        + Hash
        + Eq
        + IntoIterator<Item = Self::GenericArg>;
    type GenericArg: Copy + DebugWithInfcx<Self> + Hash + Eq;
    type Term: Copy + Debug + Hash + Eq;

    type Binder<T: TypeVisitable<Self>>: BoundVars<Self> + TypeSuperVisitable<Self>;
    type BoundVars: IntoIterator<Item = Self::BoundVar>;
    type BoundVar;

    type CanonicalVars: Copy + Debug + Hash + Eq + IntoIterator<Item = CanonicalVarInfo<Self>>;

    // Kinds of tys
    type Ty: Copy
        + DebugWithInfcx<Self>
        + Hash
        + Eq
        + Into<Self::GenericArg>
        + IntoKind<Kind = TyKind<Self>>
        + TypeSuperVisitable<Self>
        + TypeSuperFoldable<Self>
        + Flags
        + new::Ty<Self>;
    type Tys: Copy + Debug + Hash + Eq + IntoIterator<Item = Self::Ty>;
    type AliasTy: Copy + DebugWithInfcx<Self> + Hash + Eq;
    type ParamTy: Copy + Debug + Hash + Eq;
    type BoundTy: Copy + Debug + Hash + Eq;
    type PlaceholderTy: Copy + Debug + Hash + Eq + PlaceholderLike;

    // Things stored inside of tys
    type ErrorGuaranteed: Copy + Debug + Hash + Eq;
    type BoundExistentialPredicates: Copy + DebugWithInfcx<Self> + Hash + Eq;
    type PolyFnSig: Copy + DebugWithInfcx<Self> + Hash + Eq;
    type AllocId: Copy + Debug + Hash + Eq;
    type Pat: Copy + Debug + Hash + Eq + DebugWithInfcx<Self>;

    // Kinds of consts
    type Const: Copy
        + DebugWithInfcx<Self>
        + Hash
        + Eq
        + Into<Self::GenericArg>
        + IntoKind<Kind = ConstKind<Self>>
        + ConstTy<Self>
        + TypeSuperVisitable<Self>
        + TypeSuperFoldable<Self>
        + Flags
        + new::Const<Self>;
    type AliasConst: Copy + DebugWithInfcx<Self> + Hash + Eq;
    type PlaceholderConst: Copy + Debug + Hash + Eq + PlaceholderLike;
    type ParamConst: Copy + Debug + Hash + Eq;
    type BoundConst: Copy + Debug + Hash + Eq;
    type ValueConst: Copy + Debug + Hash + Eq;
    type ExprConst: Copy + DebugWithInfcx<Self> + Hash + Eq;

    // Kinds of regions
    type Region: Copy
        + DebugWithInfcx<Self>
        + Hash
        + Eq
        + Into<Self::GenericArg>
        + IntoKind<Kind = RegionKind<Self>>
        + Flags
        + new::Region<Self>;
    type EarlyParamRegion: Copy + Debug + Hash + Eq;
    type LateParamRegion: Copy + Debug + Hash + Eq;
    type BoundRegion: Copy + Debug + Hash + Eq;
    type InferRegion: Copy + DebugWithInfcx<Self> + Hash + Eq;
    type PlaceholderRegion: Copy + Debug + Hash + Eq + PlaceholderLike;

    // Predicates
    type Predicate: Copy
        + Debug
        + Hash
        + Eq
        + TypeSuperVisitable<Self>
        + TypeSuperFoldable<Self>
        + Flags;
    type TraitPredicate: Copy + Debug + Hash + Eq;
    type RegionOutlivesPredicate: Copy + Debug + Hash + Eq;
    type TypeOutlivesPredicate: Copy + Debug + Hash + Eq;
    type ProjectionPredicate: Copy + Debug + Hash + Eq;
    type NormalizesTo: Copy + Debug + Hash + Eq;
    type SubtypePredicate: Copy + Debug + Hash + Eq;
    type CoercePredicate: Copy + Debug + Hash + Eq;
    type ClosureKind: Copy + Debug + Hash + Eq;
    type Clauses: Copy + Debug + Hash + Eq + TypeSuperVisitable<Self> + Flags;

    fn mk_canonical_var_infos(self, infos: &[CanonicalVarInfo<Self>]) -> Self::CanonicalVars;
}

/// Common capabilities of placeholder kinds
pub trait PlaceholderLike {
    fn universe(self) -> UniverseIndex;
    fn var(self) -> BoundVar;

    fn with_updated_universe(self, ui: UniverseIndex) -> Self;

    fn new(ui: UniverseIndex, var: BoundVar) -> Self;
}

pub trait IntoKind {
    type Kind;

    fn kind(self) -> Self::Kind;
}

pub trait ConstTy<I: Interner> {
    fn ty(self) -> I::Ty;
}

/// Imagine you have a function `F: FnOnce(&[T]) -> R`, plus an iterator `iter`
/// that produces `T` items. You could combine them with
/// `f(&iter.collect::<Vec<_>>())`, but this requires allocating memory for the
/// `Vec`.
///
/// This trait allows for faster implementations, intended for cases where the
/// number of items produced by the iterator is small. There is a blanket impl
/// for `T` items, but there is also a fallible impl for `Result<T, E>` items.
pub trait CollectAndApply<T, R>: Sized {
    type Output;

    /// Produce a result of type `Self::Output` from `iter`. The result will
    /// typically be produced by applying `f` on the elements produced by
    /// `iter`, though this may not happen in some impls, e.g. if an error
    /// occurred during iteration.
    fn collect_and_apply<I, F>(iter: I, f: F) -> Self::Output
    where
        I: Iterator<Item = Self>,
        F: FnOnce(&[T]) -> R;
}

/// The blanket impl that always collects all elements and applies `f`.
impl<T, R> CollectAndApply<T, R> for T {
    type Output = R;

    /// Equivalent to `f(&iter.collect::<Vec<_>>())`.
    fn collect_and_apply<I, F>(mut iter: I, f: F) -> R
    where
        I: Iterator<Item = T>,
        F: FnOnce(&[T]) -> R,
    {
        // This code is hot enough that it's worth specializing for the most
        // common length lists, to avoid the overhead of `SmallVec` creation.
        // Lengths 0, 1, and 2 typically account for ~95% of cases. If
        // `size_hint` is incorrect a panic will occur via an `unwrap` or an
        // `assert`.
        match iter.size_hint() {
            (0, Some(0)) => {
                assert!(iter.next().is_none());
                f(&[])
            }
            (1, Some(1)) => {
                let t0 = iter.next().unwrap();
                assert!(iter.next().is_none());
                f(&[t0])
            }
            (2, Some(2)) => {
                let t0 = iter.next().unwrap();
                let t1 = iter.next().unwrap();
                assert!(iter.next().is_none());
                f(&[t0, t1])
            }
            _ => f(&iter.collect::<SmallVec<[_; 8]>>()),
        }
    }
}

/// A fallible impl that will fail, without calling `f`, if there are any
/// errors during collection.
impl<T, R, E> CollectAndApply<T, R> for Result<T, E> {
    type Output = Result<R, E>;

    /// Equivalent to `Ok(f(&iter.collect::<Result<Vec<_>>>()?))`.
    fn collect_and_apply<I, F>(mut iter: I, f: F) -> Result<R, E>
    where
        I: Iterator<Item = Result<T, E>>,
        F: FnOnce(&[T]) -> R,
    {
        // This code is hot enough that it's worth specializing for the most
        // common length lists, to avoid the overhead of `SmallVec` creation.
        // Lengths 0, 1, and 2 typically account for ~95% of cases. If
        // `size_hint` is incorrect a panic will occur via an `unwrap` or an
        // `assert`, unless a failure happens first, in which case the result
        // will be an error anyway.
        Ok(match iter.size_hint() {
            (0, Some(0)) => {
                assert!(iter.next().is_none());
                f(&[])
            }
            (1, Some(1)) => {
                let t0 = iter.next().unwrap()?;
                assert!(iter.next().is_none());
                f(&[t0])
            }
            (2, Some(2)) => {
                let t0 = iter.next().unwrap()?;
                let t1 = iter.next().unwrap()?;
                assert!(iter.next().is_none());
                f(&[t0, t1])
            }
            _ => f(&iter.collect::<Result<SmallVec<[_; 8]>, _>>()?),
        })
    }
}
