import { artifactSources, transferError } from './app-download.js';
import { isKnownUiModule } from './app-module-registry.js';

const stable = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/;
const hash = /^[a-f0-9]{64}$/i;
export function compareAppVersions(a, b) {
  if (!stable.test(a) || !stable.test(b)) return null;
  const left = a.split('.').map(BigInt), right = b.split('.').map(BigInt);
  for (let i = 0; i < 3; i++) if (left[i] !== right[i]) return left[i] > right[i] ? 1 : -1;
  return 0;
}

export function resolveAppPackages(entry, host) {
  if (!isKnownUiModule(entry.app_id)) return { reason: 'appsNeedsUpdate', packages: [] };
  if (!host) return { reason: 'appsHostOffline', packages: [] };
  if (entry.published === false) return { reason: 'appsNotReleased', packages: [] };
  if (entry.minNativesVersion && compareAppVersions(host.version, entry.minNativesVersion) !== 0
      && compareAppVersions(host.version, entry.minNativesVersion) !== 1) {
    return { reason: 'appsNeedsUpdate', packages: [] };
  }
  if (!entry.packages?.length) return { reason: 'appsNotReleased', packages: [] };
  const packages = entry.packages.filter((pkg) =>
    (pkg.platform === host.platform || pkg.platform === 'any') && (pkg.arch === host.arch || pkg.arch === 'any'));
  const runtimes = packages.filter((pkg) => pkg.kind === 'runtime' && pkg.required !== false);
  if (runtimes.length !== 1) return { reason: 'appsUnsupportedPlatform', packages: [] };
  const ids = new Set();
  let wire = 0, payload = 0, required = 0;
  for (const pkg of packages) {
    if (!/^[a-z0-9][a-z0-9._-]{0,127}$/.test(pkg.package_id) || ids.has(pkg.package_id)
        || !['runtime', 'data'].includes(pkg.kind) || pkg.version !== entry.version
        || !hash.test(pkg.artifact_sha256) || !hash.test(pkg.payload_sha256)
        || !Number.isSafeInteger(pkg.wire_size) || pkg.wire_size <= 0 || pkg.wire_size > 5 * 1024 * 1024
        || !Number.isSafeInteger(pkg.payload_size) || pkg.payload_size <= 0 || pkg.payload_size > 20 * 1024 * 1024) {
      throw transferError('APP_PACKAGE_INVALID', 'invalid package descriptor');
    }
    artifactSources(pkg.url);
    ids.add(pkg.package_id);
    payload += pkg.payload_size;
    if (pkg.required !== false) { required++; wire += pkg.wire_size; }
  }
  if (packages.length > 16 || required > 3 || wire > 15 * 1024 * 1024 || payload > 50 * 1024 * 1024) {
    throw transferError('APP_SIZE_LIMIT', 'app package set exceeds budget');
  }
  return { packages, reason: null };
}

export function classifyAppError(error) {
  const code = error?.code === 'internal_error'
    ? /APP_[A-Z_]+/.exec(error.message)?.[0] : error?.code;
  if (code === 'APP_CANCELLED') return 'appsCancelled';
  if (['APP_NETWORK', 'request_timeout'].includes(code)) return 'appsNetworkError';
  if (code === 'host_disconnected') return 'appsHostOffline';
  if (code === 'APP_HOST_UPDATE_REQUIRED') return 'appsNeedsUpdate';
  if (['APP_BUSY', 'APP_CONFLICT'].includes(code)) return 'appsBusy';
  if (code === 'CATALOG_SIGNATURE_INVALID' || code === 'APP_SOURCE_INVALID'
      || code === 'APP_PACKAGE_INVALID' || code === 'PACKAGE_TRANSFER_FAILED' || code === 'APP_SIZE_LIMIT') return 'appsVerificationFailed';
  return 'appsOperationFailed';
}
