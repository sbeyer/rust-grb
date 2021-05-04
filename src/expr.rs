#![allow(clippy::many_single_char_names)]
//! Algebraic expressions involving variables used to construct constraints and a helper trait for pretty-printing.

use fnv::FnvHashMap;
use std::fmt;
use std::fmt::Write;
use std::iter::Sum;
use std::ops::{Add, Mul, Neg, Sub};

use crate::prelude::*;
use crate::constr::{IneqExpr, RangeExpr};
use crate::{Error, Result};
use std::collections::HashMap;

/// An algbraic expression of variables.
#[derive(Debug, Clone)]
pub enum Expr {
    /// A quadratic expression
    Quad(QuadExpr),
    /// A linear expression
    Linear(LinExpr),
    /// A single quadratic term
    QTerm(f64, Var, Var),
    /// A single linear term
    Term(f64, Var),
    /// A constant term
    Constant(f64),
}

impl Expr {
    fn into_higher_order(self) -> Expr {
        use self::Expr::*;
        match self {
            Constant(x) => Linear(LinExpr::new()) + Constant(x),
            Term(a, x) => Linear(LinExpr::new()) + Term(a, x),
            QTerm(a, x, y) => Quad(QuadExpr::new()) + QTerm(a, x, y),
            Linear(e) => QuadExpr {
                linexpr: e,
                qcoeffs: FnvHashMap::default(),
            }
            .into(),
            Quad(_) => unreachable!(),
        }
    }

    /// Returns `false` if quadratic terms are present.
    pub fn is_linear(&self) -> bool {
        !matches!(self, Expr::QTerm(..) | Expr::Quad(..))
    }

    /// Transform into a [`QuadExpr`], possibly with no quadratic terms)
    pub fn into_quadexpr(self) -> QuadExpr {
        use self::Expr::*;
        match self {
            Quad(e) => e,
            other => other.into_higher_order().into_quadexpr(),
        }
    }

    /// Transform into a [`LinExpr`], possible with no linear terms (just a constant)
    ///
    /// # Errors
    /// Returns an [`Error::AlgebraicError`] if `Expr` is not linear.
    pub fn into_linexpr(self) -> Result<LinExpr> {
        use self::Expr::*;
        match self {
            Quad(..) | QTerm(..) => Err(Error::AlgebraicError(
                "expression contains quadratic terms".to_string(),
            )),
            Linear(e) => Ok(e),
            other => other.into_higher_order().into_linexpr(),
        }
    }

    /// Evaluate the expression, given an assignment of variable values.
    ///
    /// # Panics
    /// This function will panic if a variable in the expression is missing from the `var_values` map.
    pub fn evaluate<V: Copy + Into<f64>>(&self, var_values: &HashMap<Var, V>) -> f64 {
      use Expr::*;

      match self {
        Constant(c) => *c,
        Term(a, x) => *a * var_values[x].into(),
        QTerm(a, x, y) => *a * var_values[x].into() * var_values[y].into(),
        Linear(e) => e.evaluate(var_values),
        Quad(e) => e.evaluate(var_values),
      }
    }
}

impl Default for Expr {
    fn default() -> Self {
        Expr::Constant(0.0)
    }
}

/// Linear expression of variables
///
/// Represents an affine expression of variables: a constant term plus variables multiplied by coefficients.
///
/// A `LinExpr` object is typically created using [`Expr::into_linexpr`]. Most [`Model`] methods take
/// [`Expr`] as arguments instead of `LinExpr`, so converting to `LinExpr` is rarely needed.
#[derive(Debug, Clone, Default)]
pub struct LinExpr {
    coeff: FnvHashMap<Var, f64>,
    offset: f64,
}

/// Quadratic expression of variables
///
/// Represents an linear summation of quadratic terms, plus a linear expression.
///
/// A `QuadExpr` object is typically created using [`Expr::into_quadexpr`]. Most [`Model`] methods take
/// [`Expr`] as arguments instead of `QuadExpr`, so converting to `QuadExpr` is rarely needed.
#[derive(Debug, Clone, Default)]
pub struct QuadExpr {
    linexpr: LinExpr,
    qcoeffs: FnvHashMap<(Var, Var), f64>,
}

impl From<Var> for Expr {
    fn from(var: Var) -> Expr {
        Expr::Term(1.0, var)
    }
}

macro_rules! impl_all_primitives {
    ($macr:ident; $($args:tt),*) => {
      $macr!{f64 $(,$args)*}
      $macr!{f32 $(,$args)*}
      $macr!{u8 $(,$args)*}
      $macr!{u16 $(,$args)*}
      $macr!{u32 $(,$args)*}
      $macr!{u64 $(,$args)*}
      $macr!{usize $(,$args)*}
      $macr!{i8 $(,$args)*}
      $macr!{i16 $(,$args)*}
      $macr!{i32 $(,$args)*}
      $macr!{i64 $(,$args)*}
      $macr!{isize $(,$args)*}
    };
}

impl LinExpr {
    /// Create new an empty linear expression.
    pub fn new() -> Self {
        LinExpr::default()
    }

    /// Does this expression evaluate (close to) `0.0f64`?
    ///
    /// # Example
    /// ```
    /// assert!(grb::expr::LinExpr::new().is_empty());
    /// ```
    pub fn is_empty(&self) -> bool {
        self.offset.abs() < f64::EPSILON && self.coeff.is_empty()
    }

    /// Add a linear term into the expression.
    pub fn add_term(&mut self, coeff: f64, var: Var) -> &mut Self {
        self.coeff
            .entry(var)
            .and_modify(|c| *c += coeff)
            .or_insert(coeff);
        self
    }

    /// Add a constant into the expression.
    pub fn add_constant(&mut self, constant: f64) -> &mut Self {
        self.offset += constant;
        self
    }

    /// Get the constant offset
    pub fn get_offset(&self) -> f64 {
        self.offset
    }

    /// Set the constant offset,  returning the old one
    pub fn set_offset(&mut self, val: f64) -> f64 {
        std::mem::replace(&mut self.offset, val)
    }

    /// Get actual value of the expression.
    pub fn get_value(&self, model: &Model) -> Result<f64> {
        let coeff = self.coeff.values();
        let vals = model.get_obj_attr_batch(attr::X, self.coeff.keys().copied())?;
        let total = coeff
            .zip(vals.into_iter())
            .map(|(&a, x)| a * x)
            .sum::<f64>()
            + self.offset;
        Ok(total)
    }

    /// Decompose into variables, their coefficients and the offset, respectively.
    pub fn into_parts(self) -> (FnvHashMap<Var, f64>, f64) {
        (self.coeff, self.offset)
    }

    /// number of linear terms in the expression (excluding the constant)
    pub fn num_terms(&self) -> usize {
        self.coeff.len()
    }

    /// Returns an iterator over the terms excluding the offset (item type is `(&Var, &f64)`)
    pub fn iter_terms(&self) -> std::collections::hash_map::Iter<Var, f64> {
        self.coeff.iter()
    }

    /// Multiply expression by a scalar
    pub fn mul_scalar(&mut self, val: f64) -> &mut Self {
        self.offset *= val;
        self.coeff.iter_mut().for_each(|(_, a)| *a *= val);
        self
    }

    /// Remove variable terms whose coefficients are less than or equal to [`f64::EPSILON`].
    pub fn sparsify(&mut self) {
        self.coeff.retain(|_, a| a.abs() > f64::EPSILON);
    }


    /// Evaluate the expression, given an assignment of variable values.
    ///
    /// # Panics
    /// This function will panic if a variable in the expression is missing from the `var_values` map.
    pub fn evaluate<V: Copy + Into<f64>>(&self, var_values: &HashMap<Var, V>) -> f64 {
      self.iter_terms()
        .map(|(var, coeff)| var_values[var].into() * coeff)
        .sum::<f64>() + self.offset
    }
}

impl QuadExpr {
    /// Create a new empty quadratic expression
    pub fn new() -> Self {
        QuadExpr::default()
    }

    /// Does this expression evaluate (close to) `0.0f64`?
    pub fn is_empty(&self) -> bool {
        self.qcoeffs.is_empty() && self.linexpr.is_empty()
    }

    #[allow(clippy::type_complexity)]
    /// Decompose the expression into the quadratic terms and the remaining linear expression.
    ///
    /// The quadratic terms are returned in a hashmap mapping the non-linear term to its coefficient.
    /// The terms are simplified so the hashmap contains at most one of `(x,y)` and `(y,x)`.
    pub fn into_parts(self) -> (FnvHashMap<(Var, Var), f64>, LinExpr) {
        (self.qcoeffs, self.linexpr)
    }

    /// Add a linear term into the expression.
    pub fn add_term(&mut self, coeff: f64, var: Var) -> &mut Self {
        self.linexpr.add_term(coeff, var);
        self
    }

    /// Add a quadratic term into the expression.
    pub fn add_qterm(&mut self, coeff: f64, rowvar: Var, colvar: Var) -> &mut Self {
        if rowvar.id > colvar.id {
            // we don't bother checking the model_id here, it gets check when this object is passed to the model
            return self.add_qterm(coeff, colvar, rowvar);
        }
        self.qcoeffs
            .entry((rowvar, colvar))
            .and_modify(|c| *c += coeff)
            .or_insert(coeff);
        self
    }

    /// Add a constant into the expression.
    pub fn add_constant(&mut self, constant: f64) -> &mut Self {
        self.linexpr.add_constant(constant);
        self
    }

    /// Get the offset value (constant)
    pub fn get_offset(&self) -> f64 {
        self.linexpr.get_offset()
    }

    /// Set the constant offset,  returning the old one
    pub fn set_offset(&mut self, val: f64) -> f64 {
        self.linexpr.set_offset(val)
    }

    /// Get actual value of the expression.
    pub fn get_value(&self, model: &Model) -> Result<f64> {
        let coeff = self.qcoeffs.values();
        let rowvals = model.get_obj_attr_batch(attr::X, self.qcoeffs.keys().map(|&(_, x)| x))?;
        let colvals = model.get_obj_attr_batch(attr::X, self.qcoeffs.keys().map(|&(x, _)| x))?;
        let total = coeff
            .zip(rowvals.into_iter())
            .zip(colvals.into_iter())
            .map(|((&a, x), y)| a * x * y)
            .sum::<f64>()
            + self.linexpr.get_value(model)?;
        Ok(total)
    }

    /// Multiply expression by a scalar
    pub fn mul_scalar(&mut self, val: f64) -> &mut Self {
        self.linexpr.mul_scalar(val);
        self.qcoeffs.iter_mut().for_each(|(_, a)| *a *= val);
        self
    }

    /// Return a reference to the linear + constant part of the expression
    pub fn affine_part(&self) -> &LinExpr {
      &self.linexpr
    }

    /// number of **linear** terms in the expression (excluding the constant)
    pub fn num_terms(&self) -> usize {
        self.linexpr.num_terms()
    }

    /// Returns an iterator over the terms excluding the offset (item type is `(&Var, &f64)`)
    pub fn iter_terms(&self) -> std::collections::hash_map::Iter<Var, f64> {
        self.linexpr.iter_terms()
    }

    /// number of quadtratic terms in the expression
    pub fn num_qterms(&self) -> usize {
        self.qcoeffs.len()
    }

    /// Returns an iterator over the terms excluding the offset (item type is `(&Var, &f64)`)
    pub fn iter_qterms(&self) -> std::collections::hash_map::Iter<(Var, Var), f64> {
        self.qcoeffs.iter()
    }

    /// Remove variable terms whose coefficients are less than or equal to [`f64::EPSILON`].
    pub fn sparsify(&mut self) {
        self.linexpr.sparsify();
        self.qcoeffs.retain(|_, a| a.abs() > f64::EPSILON);
    }

    /// Evaluate the expression, given an assignment of variable values.
    ///
    /// # Panics
    /// This function will panic if a variable in the expression is missing from the `var_values` map.
    pub fn evaluate<V: Copy + Into<f64>>(&self, var_values: &HashMap<Var, V>) -> f64 {
      self.iter_qterms()
        .map(|((v1, v2), &coeff)| var_values[v1].into() * var_values[v2].into() * coeff)
        .sum::<f64>() + self.linexpr.evaluate(var_values)
    }
}

impl Add for Expr {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        use self::Expr::*;
        match (self, rhs) {
            (Constant(a), Constant(b)) => Constant(a + b),
            (Constant(c), Term(a, x)) => {
                let mut e = LinExpr::new();
                e.add_constant(c);
                e.add_term(a, x);
                e.into()
            }
            (Constant(c), QTerm(a, x, y)) => {
                let mut e = QuadExpr::new();
                e.add_qterm(a, x, y);
                e.add_constant(c);
                e.into()
            }
            (Constant(c), Linear(mut e)) => {
                e.add_constant(c);
                e.into()
            }
            (Constant(c), Quad(mut e)) => {
                e.add_constant(c);
                e.into()
            }
            (Term(a, x), Term(b, y)) => {
                let mut e = LinExpr::new();
                e.add_term(a, x);
                e.add_term(b, y);
                e.into()
            }
            (Term(a, x), QTerm(b, y1, y2)) => {
                let mut e = QuadExpr::new();
                e.add_term(a, x);
                e.add_qterm(b, y1, y2);
                e.into()
            }
            (Term(a, x), Linear(mut e)) => {
                e.add_term(a, x);
                e.into()
            }
            (Term(a, x), Quad(mut e)) => {
                e.add_term(a, x);
                e.into()
            }
            (QTerm(a, x1, x2), QTerm(b, y1, y2)) => {
                let mut e = QuadExpr::new();
                e.add_qterm(a, x1, x2);
                e.add_qterm(b, y1, y2);
                e.into()
            }
            (QTerm(a, x1, x2), Linear(e)) => {
                let mut e = QuadExpr {
                    linexpr: e,
                    qcoeffs: FnvHashMap::default(),
                };
                e.add_qterm(a, x1, x2);
                e.into()
            }
            (QTerm(a, x1, x2), Quad(mut e)) => {
                e.add_qterm(a, x1, x2);
                e.into()
            }
            (Linear(mut e1), Linear(e2)) => {
                let (coeffs, c) = e2.into_parts();
                e1.add_constant(c);
                for (x, a) in coeffs {
                    e1.add_term(a, x);
                }
                e1.into()
            }
            (Linear(le), Quad(mut qe)) => {
                qe.linexpr = (Linear(qe.linexpr) + Linear(le)).into_linexpr().unwrap();
                qe.into()
            }
            (Quad(mut e1), Quad(e2)) => {
                let (qcoeffs, linexpr) = e2.into_parts();
                for ((x, y), a) in qcoeffs {
                    e1.add_qterm(a, x, y);
                }
                e1.linexpr = (Linear(e1.linexpr) + Linear(linexpr))
                    .into_linexpr()
                    .unwrap();
                e1.into()
            }
            // swap operands
            (lhs, rhs) => rhs + lhs,
        }
    }
}

macro_rules! impl_from_prim_for_expr {
    ($t:ty) => {
        impl From<$t> for Expr {
            fn from(val: $t) -> Expr {
                Expr::Constant(val as f64)
            }
        }
    };
}

impl_all_primitives!(impl_from_prim_for_expr; );

impl From<LinExpr> for Expr {
    fn from(val: LinExpr) -> Expr {
        Expr::Linear(val)
    }
}

impl From<QuadExpr> for Expr {
    fn from(val: QuadExpr) -> Expr {
        Expr::Quad(val)
    }
}

impl<T: Copy + Into<Expr>> From<&T> for Expr {
    fn from(val: &T) -> Expr {
        (*val).into()
    }
}

impl Sub for Expr {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        self + (-rhs)
    }
}

impl Add for Var {
    type Output = Expr;
    fn add(self, rhs: Self) -> Expr {
        let lhs: Expr = self.into();
        let rhs: Expr = rhs.into();
        lhs + rhs
    }
}

impl Mul for Var {
    type Output = Expr;
    fn mul(self, rhs: Self) -> Expr {
        Expr::QTerm(1.0, self, rhs)
    }
}

impl Sub for Var {
    type Output = Expr;
    fn sub(self, rhs: Self) -> Expr {
        self + (-rhs)
    }
}

macro_rules! impl_mul_t_expr {
  ($p:ty, $($t:ty),+) => {
    impl Mul<$p> for Expr {
      type Output = Expr;
      fn mul(self, rhs: $p) -> Expr {
        use self::Expr::*;
        let rhs = rhs as f64;
        match self {
          Constant(a) => Constant(a * rhs),
          Term(a, x) => Term(a*rhs, x),
          QTerm(a, x, y) => QTerm(a*rhs, x, y),
          Linear(mut e) => {
            e.mul_scalar(rhs);
            e.into()
          }
          Quad(mut e) => {
            e.mul_scalar(rhs);
            e.into()
          }
        }
      }
    }

    impl Mul<Expr> for $p {
      type Output = Expr;
      fn mul(self, rhs: Expr) -> Expr { rhs*self }
    }

    $(
      impl Mul<$t> for $p {
        type Output = Expr;
        fn mul(self, rhs: $t) -> Expr { self * <$t as Into<Expr>>::into(rhs) }
      }

      impl Mul<$p> for $t {
        type Output = Expr;
        fn mul(self, rhs: $p) -> Expr { rhs*self }
      }

    )+
  };
}

impl_all_primitives!(impl_mul_t_expr; Var, LinExpr, QuadExpr );

macro_rules! impl_add_nonprim_expr {
  ($($t:ty),+) => {
    $(
      impl Add<$t> for Expr {
        type Output = Expr;
        fn add(self, rhs: $t) -> Expr { self + Expr::from(rhs) }
      }


      impl Add<Expr> for $t {
        type Output = Expr;
        fn add(self, rhs: Expr) -> Expr { rhs + self }
      }

    )+
  }
}

macro_rules! impl_add_prim_t {
  ($p:ty, $($t:ty),+) => {
    $(
      impl Add<$p> for $t {
        type Output = Expr;
        fn add(self, rhs: $p) -> Expr { Expr::from(self) + Expr::from(rhs) }
      }

      impl Add<$t> for $p {
        type Output = Expr;
        fn add(self, rhs: $t) -> Expr { Expr::from(rhs) + Expr::from(self) }
      }
    )+
  }
}

impl_add_nonprim_expr!(Var, LinExpr, QuadExpr);
impl_all_primitives!(impl_add_prim_t; Expr, Var, LinExpr, QuadExpr );

macro_rules! impl_sub_nonprim_expr {
    ($($t:ty),+) => {
    $(
      impl Sub<$t> for Expr {
        type Output = Expr;
        fn sub(self, rhs : $t) -> Expr { self + (-Expr::from(rhs))}
      }

      impl Sub<Expr> for $t {
        type Output = Expr;
        fn sub(self, rhs: Expr) -> Expr { Expr::from(self) + (-rhs) }
      }
    )+
  };
}

macro_rules! impl_sub_prim_t {
  ($p:ty, $($t:ty),+) => {
    $(
      impl Sub<$p> for $t {
        type Output = Expr;
        fn sub(self, rhs: $p) -> Expr { Expr::from(self) + -Expr::from(rhs) }
      }

      impl Sub<$t> for $p {
        type Output = Expr;
        fn sub(self, rhs: $t) -> Expr { Expr::from(self) + -Expr::from(rhs)  }
      }
    )+
  }
}

impl_sub_nonprim_expr!(Var, LinExpr, QuadExpr);
impl_all_primitives!(impl_sub_prim_t; Expr, Var, LinExpr, QuadExpr);

impl Neg for Var {
    type Output = Expr;
    fn neg(self) -> Expr {
        Expr::Term(-1.0, self)
    }
}

impl Neg for Expr {
    type Output = Expr;
    fn neg(self) -> Expr {
        use self::Expr::*;
        match self {
            Constant(a) => Constant(-a),
            Term(a, x) => Term(-a, x),
            QTerm(a, x, y) => QTerm(-a, x, y),
            other => -1.0 * other,
        }
    }
}

impl<A: Into<Expr>> Sum<A> for Expr {
    fn sum<I>(mut iter: I) -> Expr
    where
        I: Iterator<Item = A>,
    {
        let mut total = iter.next().map_or(Expr::Constant(0.0), |x| x.into());
        for x in iter {
            total = total + x.into();
        }
        total
    }
}

/// Deprecated trait for pretty-printing constraints.  Use [`NameMapped`] instead, whose
/// `with_names` accepts a wider variety of types instead of just [`Model`]
#[deprecated]
pub trait AttachModel {
    /// Attach a model reference to this object for formatting with [`Debug`]
    /// This trait is deprecated, use [`NameMapped`] instead.
    #[deprecated]
    fn attach<'a>(&'a self, model: &'a Model) -> Attached<'a, Self>
    where
        Self: Sized,
    {
        Attached { inner: self, model }
    }
}

#[allow(deprecated)]
impl<T: AttachVarNames> AttachModel for T {}

fn float_fmt_helper(x: f64, ignore_val: f64) -> (Option<f64>, bool) {
    let positive = x > -f64::EPSILON;
    if (x - ignore_val).abs() < f64::EPSILON {
        (None, positive)
    } else if positive {
        (Some(x), positive)
    } else {
        (Some(-x), positive)
    }
}

/// A helper trait for using [`AttachVarNames`] trait.  Any type that implements `QueryVarName` may be
/// used with [`AttachVarNames::with_names`].
pub trait QueryVarName {
  /// Write the name of the supplied `var` to the given formatter.
  fn write_name(&self, var: &Var, f: &mut fmt::Formatter<'_>) -> fmt::Result;
}

impl<V, S> QueryVarName for std::collections::HashMap<Var, V, S>
  where
    S: std::hash::BuildHasher,
    V: AsRef<str>,
{
  fn write_name(&self, var: &Var, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    let name = self.get(var).ok_or(fmt::Error)?.as_ref();
    f.write_str(name)
  }
}

impl QueryVarName for Model
{
  fn write_name(&self, var: &Var, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    let name = self.get_obj_attr(attr::VarName, var).map_err(|_| fmt::Error)?;
    f.write_str(&name)
  }
}

impl<F> QueryVarName for F
  where
    F: Fn(&Var, &mut fmt::Formatter<'_>) -> fmt::Result
{
  fn write_name(&self, var: &Var, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    self(var, f)
  }
}

/// A helper struct for pretty-printing variables, expressions and constraints
/// (see the [`AttachVarNames`] trait)
pub struct NameMapped<'a, T: ?Sized, N> {
  writer: &'a N,
  inner: &'a T
}


/// Convenience trait for summing over iterators to produce a single `Expr`.
///
/// The [`c!`](c) macro uses `Expr::from` to convert inputs to `Expr` objects.
/// Because [`Sum`] is generic over the iterator item type, this code
/// ```compile_fail
/// # use grb::prelude::*;
/// let mut model = Model::new("")?;
/// let x = add_binvar!(model)?;
/// let y = add_binvar!(model)?;
/// let z = add_binvar!(model)?;
/// let vars = [x, y, z];
/// let constraint = c!( vars.iter().sum() == 1 );
/// # Ok::<(), grb::Error>(())
/// ```
/// produces a compilation error:
/// ```console
///   | let constraint = c!( vars.iter().sum() == 1 );
///   |                      ^^^^ cannot infer type for enum `Expr`
/// ```
/// `GurobiSum` specialises the type parameter to work around this, so the following compile:
/// ```
/// # use grb::prelude::*;
/// # let mut model = Model::new("")?;
/// # let x = add_binvar!(model)?;
/// # let y = add_binvar!(model)?;
/// # let z = add_binvar!(model)?;
/// # let vars = [x, y, z];
/// let constraint = c!( vars.iter().grb_sum() == 1 );
/// # Ok::<(), grb::Error>(())
/// ```
/// Note that unlike [`Sum`], the iterator bound is [`IntoIterator`] rather than
/// [`Iterator`], so the `.iter()` call above can be replaced with a borrow:
/// ```
/// # use grb::prelude::*;
/// # let mut model = Model::new("")?;
/// # let x = add_binvar!(model)?;
/// # let y = add_binvar!(model)?;
/// # let z = add_binvar!(model)?;
/// # let vars = [x, y, z];
/// let constraint = c!( (&vars).grb_sum() == 1 );
/// # Ok::<(), grb::Error>(())
/// ```
/// This may or may not be more ergonomic.
///
/// TLDR: Use `.grb_sum()` instead of `sum()` when summing over an iterator of variables or variable expressions.
pub trait GurobiSum {
  /// Additively combine an iterator (or container) of one or more expressions into a single expression.
  fn grb_sum(self) -> Expr;
}

impl<T, I> GurobiSum for I
  where
    T: Into<Expr>,
    I: IntoIterator<Item = T>,
{
  fn grb_sum(self) -> Expr {
    self.into_iter().sum()
  }
}

/// A helper struct for pretty-printing variables, expressions and constraints
/// (see the [`AttachModel`] trait)
pub struct Attached<'a, T> {
  pub(crate) inner: &'a T,
  pub(crate) model: &'a Model,
}


/// A convenvience trait for displaying variable names with the [`Debug`] trait.
///
/// Variable names are not stored inside [`Var`] objects, but instead are queried from the
/// Gurobi C API.  This means the printing an expression with `println!("{:?}", &expr)`
/// will give some rather ugly output
/// ```
/// # use grb::prelude::*;
/// let mut model = Model::new("")?;
/// let x: Vec<_> = (0..5)
/// .map(|i| add_ctsvar!(model, name: &format!("x[{}]", i)).unwrap() )
/// .collect();
///
/// println!("{:?}", x[0]);
/// # Ok::<(), grb::Error>(())
/// ```
/// Output:
/// ```shell
/// Var { id: 0, model_id: 0 }
/// ```
/// Printing [`Expr`] and constraints this way quickly gets unreadable.
///
/// The `AttachVarNames` trait provides an `.with_names(&names)` method, with bundles the `&names` with
/// a reference to the object. The [`Debug`] trait is implemented for this bundled type ([`NameMapped`])
/// and properly queries `&names`. for variable names.  How variable names are queried is governed by
/// the [`QueryVarName`] trait.  [`QueryVarName`] is implemented for [`Model`], `HashMap<Var, String>`
/// and anything that implements `Fn(&Var, &mut std::fmt::Formatter<'_>) -> std::fmt::Result`.
///
/// Because querying the `VarName` from a `Model` of a variable can fail
/// (for example if the model hasn't been updated since the variable was
/// added or the `.with_names(...)` was  called with the wrong model), a formatting error can occur which
/// will cause a panic.
/// ```
/// # use grb::prelude::*;
/// let mut model = Model::new("")?;
/// let x: Vec<_> = (0..5)
///   .map(|i| add_ctsvar!(model, name: &format!("x[{}]", i)).unwrap() )
///   .collect();
/// model.update()?; // Important! Otherwise, the formatter will panic.
/// println!("{:?}", x[0].with_names(&model));
/// println!("{:?}", (x[0] + x[1]).with_names(&model));
/// println!("{:?}", c!( x[1..].grb_sum() >= x[0] ).with_names(&model));
/// println!("{:?}", c!( x.grb_sum() in 0..1 ).with_names(&model));
/// # Ok::<(), grb::Error>(())
/// ```
/// Output:
/// ```console
/// x[0]
/// x[1] + x[0]
/// x[4] + x[1] + x[3] + x[2] ≥ x[0]
/// x[4] + x[1] + x[0] + x[3] + x[2] ∈ [0, 1]
/// ```
/// Using different variable names stored in a `HashMap`:
/// ```
/// # use grb::prelude::*;
/// # let mut model = Model::new("")?;
/// # let x: Vec<_> = (0..5)
/// # .map(|i| add_ctsvar!(model).unwrap())
/// # .collect();
/// use std::collections::HashMap;
/// let names = ["apple", "bacon", "cat", "dog", "egg"];
/// let names : HashMap<_, _> = x.iter().copied().zip(names.iter()).collect();
/// # model.update()?;
/// println!("{:?}", x[0].with_names(&names));
/// println!("{:?}", (x[0] + x[1]).with_names(&names));
/// println!("{:?}", c!( x[1..].grb_sum() >= x[0] ).with_names(&names));
/// println!("{:?}", c!( x.grb_sum() in 0..1 ).with_names(&names));
/// # Ok::<(), grb::Error>(())
/// ```
/// Output:
/// ```console
/// apple
/// bacon + apple
/// egg + bacon + dog + cat ≥ apple
/// egg + bacon + apple + dog + cat ∈ [0, 1]
/// ```
/// Using a closure which references the `names` hashmap above:
/// ```
/// # use grb::prelude::*;
/// # let mut model = Model::new("")?;
/// # let x: Vec<_> = (0..5)
/// # .map(|i| add_ctsvar!(model).unwrap())
/// # .collect();
/// # use std::collections::HashMap;
/// # let names = ["apple", "bacon", "cat", "dog", "egg"];
/// # let names : HashMap<_, _> = x.iter().copied().zip(names.iter()).collect();
/// // Note the type annotations for the closure arguments are required
/// let name_map = |var: &Var, f: &mut std::fmt::Formatter<'_>| {
///  f.write_str(&names[var].to_uppercase())
/// };
/// # model.update()?;
/// println!("{:?}", x[0].with_names(&name_map));
/// println!("{:?}", (x[0] + x[1]).with_names(&name_map));
/// println!("{:?}", c!( x[1..].grb_sum() >= x[0] ).with_names(&name_map));
/// println!("{:?}", c!( x.grb_sum() in 0..1 ).with_names(&name_map));
/// # Ok::<(), grb::Error>(())
/// ```
/// Output:
/// ```console
/// APPLE
/// BACON + APPLE
/// EGG + BACON + DOG + CAT ≥ APPLE
/// EGG + BACON + APPLE + DOG + CAT ∈ [0, 1]
/// ```
pub trait AttachVarNames {
  /// Attach the variables names (by reference) to this object, enabling pretty-printing.
  /// `name_map` must by a type which implements [`QueryVarName`], such as [`Model`] or `HashMap<Var, String>`.
  fn with_names<'a, N: QueryVarName>(&'a self, name_map: &'a N) -> NameMapped<'a, Self, N> {
    NameMapped { inner: &self, writer: name_map }
  }
}

impl AttachVarNames for LinExpr {}
impl AttachVarNames for QuadExpr {}
impl AttachVarNames for Expr {}
impl AttachVarNames for Var {}
impl AttachVarNames for IneqExpr {}
impl AttachVarNames for RangeExpr {}


impl<W: QueryVarName> fmt::Debug for NameMapped<'_, Var, W> {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    self.writer.write_name(self.inner, f)
  }
}

impl<W> fmt::Debug for NameMapped<'_, LinExpr, W>
  where
    W: QueryVarName
{
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    if self.inner.is_empty() {
      return f.write_str("<empty LinExpr>");
    }

    let (offset, positive) = float_fmt_helper(self.inner.get_offset(), 0.0);

    let mut is_first_term = false;
    if let Some(offset) = offset {
      f.write_fmt(format_args!("{}", if positive { offset } else { -offset }))?;
    } else {
      is_first_term = true;
    }

    for (var, &coeff) in self.inner.iter_terms() {
      let (coeff, positive) = float_fmt_helper(coeff, 1.0);

      // write the operator with the previous term
      if !is_first_term {
        f.write_str(if positive { " + " } else { " - " })?;
      } else {
        is_first_term = false;
        if !positive {
          f.write_char('-')?;
        }
      }
      if let Some(coeff) = coeff {
        f.write_fmt(format_args!("{} ", coeff))?;
      }
      self.writer.write_name(var, f)?;
    }
    Ok(())
  }
}


impl<W: QueryVarName> fmt::Debug for NameMapped<'_, QuadExpr, W> {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    if self.inner.is_empty() {
      return f.write_str("<empty QuadExpr>");
    }

    let mut is_first_term = false;
    if self.inner.linexpr.is_empty() {
      is_first_term = true
    } else {
      self.inner.linexpr.with_names(self.writer).fmt(f)?;
    }

    for ((x, y), &coeff) in &self.inner.qcoeffs {
      let (coeff, positive) = float_fmt_helper(coeff, 1.0);
      if is_first_term {
        is_first_term = false;
        if !positive {
          f.write_char('-')?;
        }
      } else {
        f.write_str(if positive { " + " } else { " - " })?;
      }
      if let Some(coeff) = coeff {
        f.write_fmt(format_args!("{} ", coeff))?;
      }
      self.writer.write_name(x, f)?;
      f.write_char('*')?;
      self.writer.write_name(y, f)?;
    }
    Ok(())
  }
}

impl<W: QueryVarName> fmt::Debug for NameMapped<'_, Expr, W> {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    use Expr::*;
    match &self.inner {
      Constant(a) => {
        f.write_fmt(format_args!("{}", a))?;
      },
      Term(a, x) => {
        if (a - 1.0).abs() > f64::EPSILON {
          f.write_fmt(format_args!("{} ", a))?;
        }
        self.writer.write_name(x, f)?;
      }
      QTerm(a, x, y) => {
        if (a - 1.0).abs() > f64::EPSILON {
          f.write_fmt(format_args!("{} ", a))?;
        }
        self.writer.write_name(x, f)?;
        f.write_char('*')?;
        self.writer.write_name(y, f)?;
      }
      Linear(e) => {
        e.with_names(self.writer).fmt(f)?;
      },
      Quad(e) => {
        e.with_names(self.writer).fmt(f)?;
      },
    }
    Ok(())
  }
}


impl<W: QueryVarName> fmt::Debug for NameMapped<'_, IneqExpr, W> {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    use crate::ConstrSense::*;
    let cmp = match self.inner.sense {
      Less => "≤",
      Greater => "≥",
      Equal => "=",
    };

    self.inner.lhs.with_names(self.writer).fmt(f)?;
    f.write_fmt(format_args!(" {} ", cmp))?;
    self.inner.rhs.with_names(self.writer).fmt(f)?;
    Ok(())
  }
}


impl<W: QueryVarName> fmt::Debug for NameMapped<'_, RangeExpr, W> {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    self.inner.expr.with_names(self.writer).fmt(f)?;
    f.write_fmt(format_args!(
      " ∈ [{}, {}]",
      self.inner.lb,
      self.inner.ub
    ))
  }
}


impl From<Error> for fmt::Error {
    fn from(err: Error) -> fmt::Error {
        eprintln!("fmt error cause by: {}", err);
        fmt::Error {}
    }
}

macro_rules! impl_debug_attached {
    ($($t:path),*) => {
      $(
        impl fmt::Debug for Attached<'_, $t> {
          fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            self.inner.with_names(self.model).fmt(f)
          }
        }
      )*
    };
}

impl_debug_attached!(Expr, RangeExpr, IneqExpr, Var, LinExpr, QuadExpr);


#[allow(unused_variables)]
#[cfg(test)]
mod tests {
    use super::*;
    extern crate self as grb;

    macro_rules! make_model_with_vars {
    ($model:ident, $($var:ident),+) => {

      let mut $model = {
        let mut e = Env::empty().unwrap();
        e.set(param::OutputFlag, 0).unwrap();
        Model::with_env("test", &e.start().unwrap()).unwrap()
       };
      $(
        let $var = add_binvar!($model, name: stringify!($var)).unwrap();
      )+
      $model.update().unwrap(); // necessary to retrieve variable attributes
    }
  }

    #[test]
    fn simple() {
        make_model_with_vars!(model, x, y);
        let e: Expr = x * y + 1 + x + 2.0 * y;
        e.into_linexpr().unwrap_err(); // should be quadratic
    }

    #[test]
    fn nested() {
        make_model_with_vars!(model, x, y);
        let e = (x * y) * 3 + 2 * (x + 2.0 * y);
    }

    #[test]
    fn multiplication_commutes() {
        make_model_with_vars!(model, x, y, z);
        let _ = x - y;
        let e = y * x - x * y;
        dbg!(e.with_names(&model));
        let mut e = e.into_quadexpr();
        assert!(!e.is_empty());
        e.sparsify();
        assert!(e.is_empty());
    }

    #[test]
    fn multiplication() {
        make_model_with_vars!(model, x, y);
        let e = 2 * x;
        let e = x * x;
        let e = 2 * (x * x);
    }

    #[test]
    fn addition() {
        make_model_with_vars!(model, x, y);
        let e = 2 + x;
        let e = x + y;
        let e = x + x;
        let e = x + 2.8 * y + 2 * x;
    }

    #[test]
    fn subtraction() {
        make_model_with_vars!(model, x, y);
        let e = 2 - x;
        let mut e = (x - x).into_linexpr().unwrap();
        e.sparsify();
        assert!(e.is_empty());
        let e = 2 * x - y - x;

        let e1: Expr = 2 * x + 1.0 * y;
        let e2: Expr = 4 - 3 * y;
        let e: LinExpr = (e1 - e2).into_linexpr().unwrap();
        assert!((e.get_offset() - -4.0).abs() < f64::EPSILON);

        for (&var, &coeff) in e.iter_terms() {
            if var == x {
                assert!((coeff - 2.0) < f64::EPSILON)
            }
            if var == x {
                assert!((coeff - 4.0) < f64::EPSILON)
            }
        }
    }

    #[test]
    fn negate() {
        make_model_with_vars!(model, x);
        let q = -x;
        let y = -q;
        if let Expr::Term(a, var) = y {
            assert_eq!(x, var);
            assert_eq!(a, 1.0);
        } else {
            panic!("{:?}", y);
        }
        let q = -(x.clone() * x.clone());
        eprintln!("{:?}", q.with_names(&model));
    }

    #[test]
    fn summation() {
        make_model_with_vars!(model, x, y, z);
        let vars = [x.clone(), y.clone(), z.clone(), x.clone()];
        let e: Expr = vars.iter().cloned().sum();
        eprintln!("{:?}", &e);
        let e = e.into_linexpr().unwrap();
        assert_eq!(e.coeff.len(), 3);

        let vars = [2 * x, -y, -z, 0.2 * x];
        let e: Expr = vars.iter().cloned().sum();
        let e = e.into_linexpr().unwrap();
        assert_eq!(e.coeff.len(), 3);
    }

    #[test]
    fn linexpr_debug_fmt() {
        make_model_with_vars!(m, x, y);
        let e = 2usize * y;
        let s = format!("{:?}", e.with_names(&m));
        assert_eq!("2 y", s.to_string());
        eprintln!("{}", s);
        let e = x * y - 2.0f64 * (x * x);
        eprintln!("{:?}", e.with_names(&m));
    }


    #[test]
    fn expr_eval() {
        make_model_with_vars!(m, x, y);
        let mut var_values = HashMap::default();
        var_values.insert(x, 2);
        var_values.insert(y, 4);

        let e = Expr::from(1);
        assert_eq!(e.evaluate(&var_values), 1 as f64);

        let e = x * y;
        assert!((e.evaluate(&var_values) - 8.) <= 1e-8);

        let e : Expr = 2 * (x * y);
        assert!((e.evaluate(&var_values) - 16.) <= 1e-8);

        let e : Expr = 2 * x;
        assert!((e.evaluate(&var_values) - 4.) <= 1e-8);

        let e: Expr = 2 + x + 3*y;
        assert!((e.evaluate(&var_values) - 16.) <= 1e-8);

        let e: Expr = x + 3*(y*x) + 1;
        assert!((e.evaluate(&var_values) - 27.) <= 1e-8);
    }

    #[test]
    #[should_panic]
    fn expr_eval_missing_vars() {
        make_model_with_vars!(m, x, y);
        let mut var_values = HashMap::default();
        var_values.insert(x, 1);
        let e : Expr = x + y;
        e.evaluate(&var_values);
    }
}
