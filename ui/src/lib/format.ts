// Display helpers for the admin. All text comes from fr.json.
import { ApiError } from './api';
import { hasMessage, t } from './i18n';

/** "il y a 5 min" from Unix seconds. `null`: never. */
export function relativeTime(then: number | null, now: number): string {
  if (then === null) {
    return t('time.never');
  }
  const seconds = Math.max(0, now - then);
  if (seconds < 60) {
    return t('time.just_now');
  }
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) {
    return t('time.minutes', { n: minutes });
  }
  const hours = Math.floor(minutes / 60);
  if (hours < 48) {
    return t('time.hours', { n: hours });
  }
  return t('time.days', { n: Math.floor(hours / 24) });
}

/** French text for any error caught in the admin. */
export function errorMessage(error: unknown): string {
  if (error instanceof ApiError) {
    const key = `error.${error.code}`;
    return hasMessage(key) ? t(key) : t('error.unknown');
  }
  return t('error.unknown');
}

/** Current time in Unix seconds. */
export function nowSeconds(): number {
  return Math.floor(Date.now() / 1000);
}
