/** Types for the visual state machine editor. Mirrors Rust `AnimStateMachine`. */

export type TransitionCondition =
  | { type: "BoolEquals"; param: string; value: boolean }
  | { type: "FloatGreaterThan"; param: string; threshold: number }
  | { type: "FloatLessThan"; param: string; threshold: number }
  | { type: "ClipFinished" }
  | { type: "TriggerSet"; param: string };

export interface StateTransitionData {
  target: string;
  conditions: TransitionCondition[];
}

export interface AnimationStateData {
  clip_name: string;
  transitions: StateTransitionData[];
}

export type AnimParamData =
  | { type: "Bool"; value: boolean }
  | { type: "Float"; value: number }
  | { type: "Trigger" };

/** Editor layout info for a state node (not persisted to scene). */
export interface StateNodeLayout {
  x: number;
  y: number;
}

/** Complete state machine data for the visual editor. */
export interface StateMachineData {
  states: Record<string, AnimationStateData>;
  current_state?: string;
  parameters?: Record<string, AnimParamData>;
}
