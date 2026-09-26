import { describe, expect, it } from "vitest";
import configText from "../../src-tauri/tauri.conf.json?raw";
import capabilityText from "../../src-tauri/capabilities/default.json?raw";
import buildScript from "../../src-tauri/build.rs?raw";

const config = JSON.parse(configText);
const capability = JSON.parse(capabilityText);

describe("desktop security configuration", () => {
  it("restricts release scripts and network access without development exceptions", () => {
    const policy: string = config.app.security.csp;
    expect(policy).toContain("script-src 'self'");
    expect(policy).toContain("connect-src ipc: http://ipc.localhost;");
    expect(policy).toContain("object-src 'none'");
    expect(policy).toContain("frame-src 'none'");
    expect(policy).not.toMatch(/unsafe-eval|localhost:1420|https:|\*/);
    expect(config.app.security.devCsp).toContain("ws://localhost:1420");
  });
  it("exposes only explicit application commands to the main local window", () => {
    expect(capability.windows).toEqual(["main"]);
    expect(capability.remote).toBeUndefined();
    expect(capability.permissions).toEqual(["core:default", "allow-choose-folder", "allow-run-operation", "allow-cancel-operation", "allow-check-for-updates", "allow-install-update"]);
    expect(buildScript).toContain("AppManifest::new().commands");
    for (const command of ["choose_folder", "run_operation", "cancel_operation", "check_for_updates", "install_update"]) expect(buildScript).toContain(`"${command}"`);
  });
  it("requires signed versions and a fixed public release endpoint", () => {
    const updater = config.plugins.updater;
    expect(updater.endpoints).toEqual(["https://github.com/nolllerrr/siftly/releases/latest/download/latest.json"]);
    expect(updater.requireSignedVersion).toBe(true);
    expect(updater.pubkey.length).toBeGreaterThan(80);
    expect(updater.dangerousInsecureTransportProtocol).toBeFalsy();
    expect(updater.dangerousAcceptInvalidCerts).toBeFalsy();
    expect(updater.dangerousAcceptInvalidHostnames).toBeFalsy();
    expect(updater.allowDowngrades).toBeFalsy();
    expect(config.bundle.createUpdaterArtifacts).toBe(true);
  });
});
