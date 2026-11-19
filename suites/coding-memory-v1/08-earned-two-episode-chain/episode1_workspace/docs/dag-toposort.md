# Workflow DAG Topological Ordering

Dependent job stages execute in Kahn topological order; cycle detection runs at
workflow registration time prior to cadence admission.
