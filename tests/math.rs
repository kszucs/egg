use egg::*;

use ordered_float::NotNan;

pub type EGraph = egg::EGraph<Math, Meta>;
pub type Rewrite = egg::Rewrite<Math, Meta>;

type Constant = NotNan<f64>;

define_language! {
    pub enum Math {
        Constant(Constant),
        Add = "+",
        Sub = "-",
        Mul = "*",
        Div = "/",
        Pow = "pow",
        Exp = "exp",
        Log = "log",
        Sqrt = "sqrt",
        Cbrt = "cbrt",
        Fabs = "fabs",

        Log1p = "log1p",
        Expm1 = "expm1",

        RealToPosit = "real->posit",
        Variable(String),
    }
}

#[derive(Debug, Clone)]
pub struct Meta {
    pub cost: usize,
    pub best: RecExpr<Math>,
}

fn eval(op: Math, args: &[Constant]) -> Option<Constant> {
    let a = |i| args.get(i).cloned();
    match op {
        Math::Add => Some(a(0)? + a(1)?),
        Math::Sub => Some(a(0)? - a(1)?),
        Math::Mul => Some(a(0)? * a(1)?),
        Math::Div => Some(a(0)? / a(1)?),
        _ => None,
    }
}

impl Metadata<Math> for Meta {
    type Error = std::convert::Infallible;
    fn merge(&self, other: &Self) -> Self {
        if self.cost <= other.cost {
            self.clone()
        } else {
            other.clone()
        }
    }

    fn make(expr: ENode<Math, &Self>) -> Self {
        let expr = {
            let const_args: Option<Vec<Constant>> = expr
                .children
                .iter()
                .map(|meta| match meta.best.as_ref().op {
                    Math::Constant(c) => Some(c),
                    _ => None,
                })
                .collect();

            const_args
                .and_then(|a| eval(expr.op.clone(), &a))
                .map(|c| ENode::leaf(Math::Constant(c)))
                .unwrap_or(expr)
        };

        let best: RecExpr<_> = expr.map_children(|c| c.best.clone()).into();
        let cost = AstSize.cost(&expr.map_children(|c| c.cost));
        Self { best, cost }
    }

    fn modify(eclass: &mut EClass<Math, Self>) {
        // NOTE pruning vs not pruning is decided right here
        let best = eclass.metadata.best.as_ref();
        if best.children.is_empty() {
            eclass.nodes = vec![ENode::leaf(best.op.clone())]
        }
    }
}

#[rustfmt::skip]
pub fn rules() -> Vec<Rewrite> { vec![
    rw("comm-add").p("(+ ?a ?b)").a("(+ ?b ?a)").mk(),
    rw("comm-mul").p("(* ?a ?b)").a("(* ?b ?a)").mk(),
    rw("assoc-add").p("(+ ?a (+ ?b ?c))").a("(+ (+ ?a ?b) ?c)").mk(),
    rw("assoc-mul").p("(* ?a (* ?b ?c))").a("(* (* ?a ?b) ?c)").mk(),
    rw("canon-sub").p("(- ?a ?b)").a("(+ ?a (- 0 ?b))").mk(),
    rw("canon-div").p("(/ ?a ?b)").a("(* ?a (/ 1 ?b))").mk(),

    rw("zero-add").p("(+ ?a 0)").a("?a").mk(),
    rw("zero-mul").p("(* ?a 0)").a("0").mk(),
    rw("one-mul").p("(* ?a 1)").a("?a").mk(),

    rw("add-zero").p("?a").a("(+ ?a 0)").mk(),
    rw("mul-one").p("?a").a("(* ?a 1)").mk(),

    rw("cancel-sub").p("(- ?a ?a)").a("0").mk(),
    rw("cancel-div").p("(/ ?a ?a)").a("1").mk(),

    rw("negate").p("(- 0 ?a)").a("(* -1 ?a)").mk(),

    rw("distribute").p("(* ?a (+ ?b ?c))").a("(+ (* ?a ?b) (* ?a ?c))").mk(),
    rw("factor").p("(+ (* ?a ?b) (* ?a ?c))").a("(* ?a (+ ?b ?c))").mk(),
    rw("sqrt-cancel").p("(* (sqrt ?a) (sqrt ?a))").a("?a").mk(),
]}

#[test]
#[cfg_attr(feature = "parent-pointers", ignore)]
fn associate_adds() {
    let start = "(+ 1 (+ 2 (+ 3 (+ 4 (+ 5 (+ 6 7))))))";
    let start_expr = start.parse().unwrap();

    let rules = &[
        rw("comm-add").p("(+ ?a ?b)").a("(+ ?b ?a)").mk(),
        rw("assoc-add")
            .p("(+ ?a (+ ?b ?c))")
            .a("(+ (+ ?a ?b) ?c)")
            .mk(),
    ];

    // Must specfify the () metadata so pruning doesn't mess us up here
    let egraph: egg::EGraph<Math, ()> = SimpleRunner::default()
        .with_iter_limit(7)
        .with_node_limit(8_000)
        .run_expr(start_expr, rules)
        .0;

    // there are exactly 127 non-empty subsets of 7 things
    assert_eq!(egraph.number_of_classes(), 127);
}

macro_rules! check {
    (
        $(#[$meta:meta])*
        $name:ident, $iters:literal, $limit:literal,
        $start:literal => $end:literal
    ) => {
        $(#[$meta])*
        #[test]
        fn $name() {
            let _ = env_logger::builder().is_test(true).try_init();
            let start_expr = $start.parse().expect(concat!("Failed to parse ", $start));
            let end_expr = $end.parse().expect(concat!("Failed to parse ", $end));

            let (mut egraph, root) = EGraph::from_expr(&start_expr);
            let (_, reason) = SimpleRunner::default()
                .with_iter_limit($iters)
                .with_node_limit($limit)
                .run(&mut egraph, &rules());

            println!("Stopped because {:?}", reason);
            let (cost, best) = Extractor::new(&egraph, AstSize).find_best(root);
            println!("Best ({}): {}", cost, best.to_sexp());

            // make sure that pattern search also works
            let pattern = Pattern::from_expr(&end_expr);
            let matches = pattern.search_eclass(&egraph, root);

            if matches.is_none() {
                println!("start: {}", start_expr.to_sexp());
                println!("start: {:?}", start_expr);
                panic!(
                    "Could not simplify {} to {}, found:\n{}",
                    $start,
                    $end,
                    best.pretty(40)
                );
            }
        }
    };
}

check!(
    #[should_panic(expected = "Could not simplify")]
    simplify_fail, 5, 1_000, "(+ x y)" => "(/ x y)"
);

check!(
    #[cfg_attr(feature = "parent-pointers", ignore)]
    simplify_add,   20,  1_000, "(+ x (+ x (+ x x)))" => "(* 4 x)"
);
check!(
    #[cfg_attr(feature = "parent-pointers", ignore)]
    simplify_const,  4,  1_000, "(+ 1 (- a (* (- 2 1) a)))" => "1"
);
check!(
    #[cfg_attr(feature = "parent-pointers", ignore)]
    simplify_root,  10, 75_000, r#"
          (/ 1
             (- (/ (+ 1 (sqrt five))
                   2)
                (/ (- 1 (sqrt five))
                   2)))
        "#
       => "(/ 1 (sqrt five))"
);
