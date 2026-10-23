// Fill the test lab's live ops service with what scene 226 shows: a value
// that differs from the game's default, a rule, an experiment, an event on
// now and one about to start - on every channel, so a built game (`live`)
// and the editor's Play (`dev`) both see it.
//
//   node projects/engine-test-lab/liveops/seed.mjs [http://127.0.0.1:9940] [token]
const base = process.argv[2] || "http://127.0.0.1:9940";
const token = process.argv[3] || "lab";
const now = Math.floor(Date.now() / 1000);
const auth = { Authorization: `Bearer ${token}`, "Content-Type": "application/json" };

const overview = await (await fetch(`${base}/admin/api/liveops`, { headers: auth })).json();
for (const { name } of overview.channels) {
  const current = await (await fetch(`${base}/admin/api/liveops/channel/${name}`, { headers: auth })).json();
  const body = {
    revision: current.channel.revision || 0,
    params: {
      lab226_greeting: {
        value: `Hello from the ${name} channel`, description: "scene 226's greeting",
        rules: [{ name: "in Finnish", when: { languages: ["fi"] }, value: `Terve, ${name}-kanavalta` }],
      },
      lab226_said: {
        value: "the service did not hear lab=226", description: "what scene 226's attribute reached",
        rules: [{ name: "the lab's attribute", when: { attributes: { lab: ["226"] } }, value: "the service heard lab=226" }],
      },
      lab226_speed: { value: 2.5, rules: [{ name: "phones spin slower", when: { platforms: ["android", "ios"] }, value: 1.0 }] },
      lab226_tint: { value: [0.45, 0.85, 0.6] },
    },
    experiments: [{
      id: "lab226_shape", status: "running", description: "square or diamond",
      variants: [{ name: "square", weight: 1, values: {} }, { name: "diamond", weight: 1, values: {} }],
    }],
    events: [
      { id: "lab226_weekend", name: "Faster weekend", start: now - 60, end: now + 3600, values: { lab226_speed: 5.0 } },
      { id: "lab226_soon", name: "Coming soon", start: now + 600, end: now + 7200, values: {} },
    ],
  };
  const res = await fetch(`${base}/admin/api/liveops/channel/${name}`, { method: "PUT", headers: auth, body: JSON.stringify(body) });
  const out = await res.json();
  if (!res.ok) throw new Error(`${name}: ${out.error}`);
  console.log(`${name}: revision ${out.channel.revision}`);
}
