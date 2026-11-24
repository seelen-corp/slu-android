// Publishes an Android App Bundle to a Google Play track (no dependencies,
// Node 20+). Idempotent per versionCode: Play accepts each versionCode only
// once, but an uploaded bundle can be assigned to any number of tracks. So if
// the bundle of this commit was already uploaded (e.g. by the nightly), the
// upload is skipped and the existing versionCode is assigned to the track.
//
// Env:
//   PLAY_SERVICE_ACCOUNT_JSON  service account key (JSON)
//   PLAY_PACKAGE_NAME          application id
//   PLAY_AAB                   path to the .aab
//   PLAY_VERSION_CODE          versionCode of that .aab (commit count)
//   PLAY_TRACK                 internal | alpha | beta | production | <custom>
//   PLAY_RELEASE_NAME          release name shown in Play Console
//   PLAY_STATUS                draft | completed

import { createSign } from "node:crypto";
import { readFileSync } from "node:fs";

const env = (name) => {
  const value = process.env[name];
  if (!value) throw new Error(`${name} is not set`);
  return value;
};

const account = JSON.parse(env("PLAY_SERVICE_ACCOUNT_JSON"));
const packageName = env("PLAY_PACKAGE_NAME");
const aabPath = env("PLAY_AAB");
const versionCode = Number(env("PLAY_VERSION_CODE"));
const track = env("PLAY_TRACK");
const releaseName = env("PLAY_RELEASE_NAME");
const status = env("PLAY_STATUS");

const API =
  `https://androidpublisher.googleapis.com/androidpublisher/v3/applications/${packageName}`;
const UPLOAD_API =
  `https://androidpublisher.googleapis.com/upload/androidpublisher/v3/applications/${packageName}`;

async function getAccessToken() {
  const now = Math.floor(Date.now() / 1000);
  const b64 = (obj) => Buffer.from(JSON.stringify(obj)).toString("base64url");
  const unsigned = `${b64({ alg: "RS256", typ: "JWT" })}.${
    b64({
      iss: account.client_email,
      scope: "https://www.googleapis.com/auth/androidpublisher",
      aud: account.token_uri ?? "https://oauth2.googleapis.com/token",
      iat: now,
      exp: now + 3600,
    })
  }`;
  const signature = createSign("RSA-SHA256").update(unsigned).sign(
    account.private_key,
    "base64url",
  );

  const res = await fetch(
    account.token_uri ?? "https://oauth2.googleapis.com/token",
    {
      method: "POST",
      headers: { "content-type": "application/x-www-form-urlencoded" },
      body: new URLSearchParams({
        grant_type: "urn:ietf:params:oauth:grant-type:jwt-bearer",
        assertion: `${unsigned}.${signature}`,
      }),
    },
  );
  if (!res.ok) {
    throw new Error(
      `OAuth token request failed: ${res.status} ${await res.text()}`,
    );
  }
  return (await res.json()).access_token;
}

const token = await getAccessToken();

async function call(method, url, { body, headers } = {}) {
  const res = await fetch(url, {
    method,
    headers: { authorization: `Bearer ${token}`, ...headers },
    body,
  });
  const text = await res.text();
  if (!res.ok) {
    const error = new Error(`${method} ${url} failed: ${res.status} ${text}`);
    error.body = text;
    throw error;
  }
  return text ? JSON.parse(text) : {};
}

const json = (value) => ({
  body: JSON.stringify(value),
  headers: { "content-type": "application/json" },
});

const { id: editId } = await call("POST", `${API}/edits`, json({}));
const EDIT = `${API}/edits/${editId}`;

try {
  const { bundles = [] } = await call("GET", `${EDIT}/bundles`);
  if (bundles.some((b) => b.versionCode === versionCode)) {
    console.log(
      `versionCode ${versionCode} is already on Play: skipping the upload`,
    );
  } else {
    console.log(`Uploading ${aabPath} (versionCode ${versionCode})`);
    try {
      const bundle = await call(
        "POST",
        `${UPLOAD_API}/edits/${editId}/bundles?uploadType=media`,
        {
          body: readFileSync(aabPath),
          headers: { "content-type": "application/octet-stream" },
        },
      );
      if (bundle.versionCode !== versionCode) {
        throw new Error(
          `uploaded bundle has versionCode ${bundle.versionCode}, expected ${versionCode}`,
        );
      }
    } catch (error) {
      // Another run uploaded it between the listing and the upload.
      if (!/already been used/i.test(error.body ?? "")) throw error;
      console.log(
        `versionCode ${versionCode} was uploaded meanwhile: skipping the upload`,
      );
    }
  }

  const { releases = [] } = await call("GET", `${EDIT}/tracks/${track}`).catch(
    (error) => {
      if (/404/.test(error.message)) return {};
      throw error;
    },
  );
  const codesOnTrack = releases.flatMap((r) =>
    (r.versionCodes ?? []).map(Number)
  );
  if (codesOnTrack.includes(versionCode)) {
    console.log(
      `versionCode ${versionCode} is already on the '${track}' track: nothing to do`,
    );
    await call("DELETE", EDIT);
    process.exit(0);
  }
  const newest = Math.max(0, ...codesOnTrack);
  if (newest > versionCode) {
    // An older run finishing after a newer one must not downgrade the track.
    console.log(
      `'${track}' already has a newer versionCode (${newest}): not assigning ${versionCode}`,
    );
    await call("DELETE", EDIT);
    process.exit(0);
  }

  await call(
    "PUT",
    `${EDIT}/tracks/${track}`,
    json({
      track,
      releases: [{
        name: releaseName,
        versionCodes: [String(versionCode)],
        status,
      }],
    }),
  );
  await call("POST", `${EDIT}:commit`);
  console.log(
    `versionCode ${versionCode} assigned to '${track}' as '${releaseName}' (${status})`,
  );
} catch (error) {
  await call("DELETE", EDIT).catch(() => {});
  throw error;
}
