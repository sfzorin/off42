use super::*;
fn model(objective: (usize, usize), variables: Vec<(usize, usize)>) -> SolverModel {
    SolverModel {
        objective,
        variables,
        goal: SolverGoal::Maximum,
        constraints: vec![],
        non_negative: true,
        linear: true,
        extra: BTreeMap::new(),
    }
}
fn constraint(cell: (usize, usize), op: SolverOp, rhs: &str) -> SolverConstraint {
    SolverConstraint {
        lhs: Range {
            top: cell.0,
            left: cell.1,
            bottom: cell.0,
            right: cell.1,
        },
        op,
        rhs: rhs.into(),
    }
}
fn solve(book: &Workbook, m: SolverModel) -> SolverResult {
    book.solver_task(m)
        .unwrap()
        .solve(&AtomicBool::new(false))
        .unwrap()
}
fn textbook() -> (Workbook, SolverModel) {
    let mut b = Workbook::new(5, 5).unwrap();
    for (r, c, v) in [
        (0, 0, "0"),
        (0, 1, "0"),
        (0, 2, "=3*A1+5*B1"),
        (1, 0, "=A1"),
        (2, 0, "=2*B1"),
        (3, 0, "=3*A1+2*B1"),
    ] {
        b.set_cell(r, c, v.into()).unwrap();
    }
    let mut m = model((0, 2), vec![(0, 0), (0, 1)]);
    m.constraints = vec![
        constraint((1, 0), SolverOp::Le, "4"),
        constraint((2, 0), SolverOp::Le, "12"),
        constraint((3, 0), SolverOp::Le, "18"),
    ];
    (b, m)
}
#[test]
fn textbook_linear_trials_are_readonly_accept_and_reject_keep_model_with_one_undo() {
    let (mut b, m) = textbook();
    let source = b.history.current().clone();
    let id = b.history.current_id();
    let r = solve(&b, m.clone());
    assert_eq!(r.status(), SolverStatus::Optimal);
    assert_eq!(r.engine(), "simplex");
    assert!((r.values()[0] - 2.).abs() < 1e-8 && (r.values()[1] - 6.).abs() < 1e-8);
    assert!((r.objective().unwrap() - 36.).abs() < 1e-8);
    assert!(b.history.current() == &source);
    assert_eq!(b.history.current_id(), id);
    b.apply_solver_result(&r, false).unwrap();
    assert_eq!(b.cell(0, 0), Some("0"));
    assert_eq!(b.solver_model(), Some(&m));
    b.undo();
    assert!(b.history.current() == &source);
    let r = solve(&b, m.clone());
    b.apply_solver_result(&r, true).unwrap();
    let changed = b.history.current().clone();
    assert_eq!(b.cell(0, 0), Some("2"));
    assert_eq!(b.cell(0, 1), Some("6"));
    assert_eq!(b.cell(0, 2), Some("=3*A1+5*B1"));
    let reopened = Workbook::from_project_bytes(&b.project_bytes().unwrap()).unwrap();
    assert_eq!(reopened.solver_model(), Some(&m));
    assert_eq!(reopened.cell(0, 1), Some("6"));
    b.undo();
    assert!(b.history.current() == &source);
    b.redo();
    assert!(b.history.current() == &changed);
}
#[test]
fn integer_binary_free_variables_rhs_cells_and_target_modes() {
    let mut b = Workbook::new(3, 4).unwrap();
    b.set_cell(0, 0, "0".into()).unwrap();
    b.set_cell(0, 1, "=A1".into()).unwrap();
    b.set_cell(0, 2, "2.5".into()).unwrap();
    let mut m = model((0, 1), vec![(0, 0)]);
    m.constraints = vec![
        constraint((0, 0), SolverOp::Le, "C1"),
        constraint((0, 0), SolverOp::Integer, ""),
    ];
    let r = solve(&b, m.clone());
    assert_eq!(r.status(), SolverStatus::Optimal);
    assert_eq!(r.values(), &[2.]);
    m.constraints[1].op = SolverOp::Binary;
    assert_eq!(solve(&b, m.clone()).values(), &[1.]);
    m.constraints = vec![constraint((0, 0), SolverOp::Ge, "-3")];
    m.non_negative = false;
    m.goal = SolverGoal::Minimum;
    assert_eq!(solve(&b, m.clone()).values(), &[-3.]);
    m.constraints.clear();
    m.goal = SolverGoal::Value(-7.);
    assert_eq!(solve(&b, m).values(), &[-7.]);
}
#[test]
fn infeasible_unbounded_and_fractional_integer_unbounded_relaxation_are_distinct() {
    let mut b = Workbook::new(2, 3).unwrap();
    b.set_cell(0, 2, "=B1".into()).unwrap();
    let mut m = model((0, 2), vec![(0, 0), (0, 1)]);
    assert_eq!(solve(&b, m.clone()).status(), SolverStatus::Unbounded);
    m.constraints = vec![
        constraint((0, 0), SolverOp::Eq, "0.5"),
        constraint((0, 0), SolverOp::Integer, ""),
    ];
    let r = solve(&b, m);
    assert_eq!(r.status(), SolverStatus::Infeasible);
    assert!(!r.can_keep());
    assert!(b.apply_solver_result(&r, true).is_err());
}
#[test]
fn nonlinear_minimum_constraints_target_and_linear_fallback() {
    let mut b = Workbook::new(3, 3).unwrap();
    b.set_cell(0, 2, "=(A1-3)^2+(B1-2)^2".into()).unwrap();
    let mut m = model((0, 2), vec![(0, 0), (0, 1)]);
    m.goal = SolverGoal::Minimum;
    let r = solve(&b, m.clone());
    assert_eq!(r.status(), SolverStatus::Approximate);
    assert_eq!(r.engine(), "Nelder–Mead");
    assert!(r.objective().unwrap() < 1e-12, "{:?}", r.values());
    m.linear = false;
    m.constraints = vec![constraint((0, 0), SolverOp::Le, "1")];
    let r = solve(&b, m);
    assert!(r.can_keep(), "{:?} {:?}", r.status(), r.values());
    assert!((r.values()[0] - 1.).abs() < 1e-5);
    assert!((r.values()[1] - 2.).abs() < 1e-4);
    b.set_cell(0, 2, "=A1^2".into()).unwrap();
    let mut m = model((0, 2), vec![(0, 0)]);
    m.goal = SolverGoal::Value(2.);
    m.linear = false;
    let r = solve(&b, m);
    assert!(r.can_keep());
    assert!((r.values()[0] - 2f64.sqrt()).abs() < 1e-6);
}
#[test]
fn stale_cancel_protection_and_final_validation_never_partially_apply_variables() {
    let (mut b, m) = textbook();
    let source = b.history.current().clone();
    assert!(b
        .solver_task(m.clone())
        .unwrap()
        .solve(&AtomicBool::new(true))
        .is_err());
    assert!(b.history.current() == &source);
    let r = solve(&b, m.clone());
    b.set_cell(4, 4, "changed".into()).unwrap();
    let source = b.history.current().clone();
    assert!(b.apply_solver_result(&r, true).is_err());
    assert!(b.history.current() == &source);
    b.add_validation(DataValidation {
        range: Range {
            top: 0,
            left: 1,
            bottom: 0,
            right: 1,
        },
        allow_blank: true,
        kind: ValidationKind::Number {
            minimum: 0.,
            maximum: 1.,
        },
    })
    .unwrap();
    let r = solve(&b, m.clone());
    let source = b.history.current().clone();
    assert!(b.apply_solver_result(&r, true).is_err());
    assert!(b.history.current() == &source);
    assert_eq!(b.cell(0, 0), Some("0"));
    b.set_protected(true);
    assert!(b.solver_task(m).is_err());
}
#[test]
fn office_models_are_native_scoped_names_without_private_metadata_and_editable() {
    let (mut b, mut m) = textbook();
    m.extra
        .insert("solver_custom_option".into(), "\"keep & <this>\"".into());
    b.rename_sheet(b.active_sheet(), "Bob's, Sheet α").unwrap();
    b.set_solver_model(Some(m.clone())).unwrap();
    for kind in ["xlsx", "ods"] {
        let bytes = if kind == "xlsx" {
            b.xlsx_bytes()
        } else {
            b.ods_bytes()
        }
        .unwrap();
        // Force the independent Office import path, which ignores the embedded private project.
        let mut external = Workbook::from_external_office(&bytes, kind).unwrap();
        let decoded = external.solver_model().unwrap();
        assert_eq!(decoded.objective, m.objective);
        assert_eq!(decoded.variables, m.variables);
        assert_eq!(decoded.constraints, m.constraints);
        assert_eq!(
            decoded.extra.get("solver_custom_option"),
            m.extra.get("solver_custom_option")
        );
        let mut edited = decoded.clone();
        edited.goal = SolverGoal::Minimum;
        edited.non_negative = false;
        external.set_solver_model(Some(edited.clone())).unwrap();
        let output = if kind == "xlsx" {
            external.xlsx_bytes()
        } else {
            external.ods_bytes()
        }
        .unwrap();
        let mut restored = Workbook::from_external_office(&output, kind).unwrap();
        assert_eq!(restored.solver_model(), Some(&edited));
        assert_eq!(restored.sheets()[0].1.as_str(), "Bob's, Sheet α");
        restored.set_solver_model(None).unwrap();
        let output = if kind == "xlsx" {
            restored.xlsx_bytes()
        } else {
            restored.ods_bytes()
        }
        .unwrap();
        assert!(Workbook::from_external_office(&output, kind)
            .unwrap()
            .solver_model()
            .is_none());
    }
}
#[test]
fn structural_edits_shift_solver_objective_variables_constraints_rhs_and_undo() {
    let (mut b, mut m) = textbook();
    m.constraints[0].rhs = "$D$2".into();
    b.set_solver_model(Some(m.clone())).unwrap();
    b.insert_rows(0, 2).unwrap();
    let shifted = b.solver_model().unwrap();
    assert_eq!(shifted.objective, (2, 2));
    assert_eq!(shifted.variables, vec![(2, 0), (2, 1)]);
    assert_eq!(shifted.constraints[0].lhs.top, 3);
    assert_eq!(shifted.constraints[0].rhs, "$D$4:$D$4");
    b.undo();
    assert_eq!(b.solver_model(), Some(&m));
    b.redo();
    b.delete_rows(2, 1).unwrap();
    assert!(b.solver_model().is_none());
}
#[test]
fn metadata_only_change_preserves_foreign_names_and_unsupported_formula_cache() {
    use std::io::Read;
    let mut source = rust_xlsxwriter::Workbook::new();
    source
        .add_worksheet()
        .set_name("Data")
        .unwrap()
        .write_number(0, 0, 1.)
        .unwrap()
        .write_number(0, 1, 0.)
        .unwrap()
        .write_formula(
            0,
            2,
            rust_xlsxwriter::Formula::new("=UNSUPPORTED42(A1)").set_result("42"),
        )
        .unwrap();
    source.define_name("ForeignName", "=Data!$A$1").unwrap();
    source.define_name("Data!LocalName", "=Data!$B$1").unwrap();
    source.define_name("solver_global", "=99").unwrap();
    let bytes = source.save_to_buffer().unwrap();
    let mut b = Workbook::from_office_bytes(&bytes, "xlsx").unwrap();
    assert_eq!(b.calculated_grid()[0][2], "42");
    let mut m = model((0, 0), vec![(0, 1)]);
    m.goal = SolverGoal::Minimum;
    b.set_solver_model(Some(m)).unwrap();
    assert_eq!(b.calculated_grid()[0][2], "42");
    let output = b.xlsx_bytes().unwrap();
    let mut before = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut after = zip::ZipArchive::new(std::io::Cursor::new(output)).unwrap();
    for name in before
        .file_names()
        .filter(|n| *n != "xl/workbook.xml")
        .map(str::to_owned)
        .collect::<Vec<_>>()
    {
        let mut a = Vec::new();
        let mut c = Vec::new();
        before.by_name(&name).unwrap().read_to_end(&mut a).unwrap();
        after.by_name(&name).unwrap().read_to_end(&mut c).unwrap();
        assert_eq!(a, c, "{name}");
    }
    let mut xml = String::new();
    after
        .by_name("xl/workbook.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    let doc = office::xml(&xml).unwrap();
    for name in ["ForeignName", "LocalName", "solver_global"] {
        assert_eq!(
            doc.descendants()
                .filter(|n| n.has_tag_name("definedName") && n.attribute("name") == Some(name))
                .count(),
            1
        );
    }
}
