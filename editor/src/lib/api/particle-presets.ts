/** Particle emitter presets — each preset defines a full set of emitter fields. */

export interface ParticlePreset {
  label: string;
  values: {
    emission_rate: number;
    max_particles: number;
    lifetime: [number, number];
    speed: [number, number];
    angle: [number, number];
    color_start: [number, number, number, number];
    color_end: [number, number, number, number];
    size_start: [number, number];
    size_end: [number, number];
    gravity_modifier: number;
    angular_velocity: [number, number];
    burst_count: number;
    active: boolean;
    world_space: boolean;
    z_order: number;
  };
}

export const PARTICLE_PRESETS: ParticlePreset[] = [
  {
    label: "Fire",
    values: {
      emission_rate: 40,
      max_particles: 512,
      lifetime: [0.4, 1.0],
      speed: [30, 80],
      angle: [4.4, 5.0],
      color_start: [1.0, 0.6, 0.1, 1.0],
      color_end: [0.8, 0.2, 0.0, 0.0],
      size_start: [10, 16],
      size_end: [2, 4],
      gravity_modifier: -0.3,
      angular_velocity: [-1.0, 1.0],
      burst_count: 0,
      active: true,
      world_space: true,
      z_order: 0,
    },
  },
  {
    label: "Smoke",
    values: {
      emission_rate: 15,
      max_particles: 256,
      lifetime: [1.0, 3.0],
      speed: [10, 30],
      angle: [4.4, 5.0],
      color_start: [0.5, 0.5, 0.5, 0.6],
      color_end: [0.3, 0.3, 0.3, 0.0],
      size_start: [8, 14],
      size_end: [20, 30],
      gravity_modifier: -0.1,
      angular_velocity: [-0.5, 0.5],
      burst_count: 0,
      active: true,
      world_space: true,
      z_order: 0,
    },
  },
  {
    label: "Explosion",
    values: {
      emission_rate: 0,
      max_particles: 128,
      lifetime: [0.3, 0.8],
      speed: [100, 250],
      angle: [0, 6.2832],
      color_start: [1.0, 0.8, 0.2, 1.0],
      color_end: [0.6, 0.1, 0.0, 0.0],
      size_start: [6, 12],
      size_end: [1, 2],
      gravity_modifier: 0.5,
      angular_velocity: [-3.0, 3.0],
      burst_count: 64,
      active: true,
      world_space: true,
      z_order: 0,
    },
  },
  {
    label: "Sparkle",
    values: {
      emission_rate: 25,
      max_particles: 200,
      lifetime: [0.3, 0.8],
      speed: [20, 60],
      angle: [0, 6.2832],
      color_start: [1.0, 1.0, 0.8, 1.0],
      color_end: [0.8, 0.8, 1.0, 0.0],
      size_start: [3, 6],
      size_end: [1, 2],
      gravity_modifier: 0,
      angular_velocity: [-2.0, 2.0],
      burst_count: 0,
      active: true,
      world_space: false,
      z_order: 0,
    },
  },
  {
    label: "Rain",
    values: {
      emission_rate: 80,
      max_particles: 1024,
      lifetime: [0.8, 1.5],
      speed: [200, 350],
      angle: [1.4, 1.7],
      color_start: [0.6, 0.7, 1.0, 0.8],
      color_end: [0.5, 0.6, 0.9, 0.2],
      size_start: [1, 2],
      size_end: [1, 2],
      gravity_modifier: 1.0,
      angular_velocity: [0, 0],
      burst_count: 0,
      active: true,
      world_space: true,
      z_order: 0,
    },
  },
  {
    label: "Snow",
    values: {
      emission_rate: 30,
      max_particles: 512,
      lifetime: [2.0, 4.0],
      speed: [15, 40],
      angle: [1.2, 1.9],
      color_start: [1.0, 1.0, 1.0, 0.9],
      color_end: [0.9, 0.95, 1.0, 0.0],
      size_start: [3, 6],
      size_end: [2, 4],
      gravity_modifier: 0.2,
      angular_velocity: [-1.0, 1.0],
      burst_count: 0,
      active: true,
      world_space: true,
      z_order: 0,
    },
  },
];
