import { describe, expect, it } from 'vitest';

import { ApiError } from './api';
import { errorMessage, relativeTime } from './format';

describe('relativeTime', () => {
  const now = 1_800_000_000;

  it('says never without a date', () => {
    expect(relativeTime(null, now)).toBe('jamais');
  });

  it('rounds down to the unit', () => {
    expect(relativeTime(now - 59, now)).toBe("à l'instant");
    expect(relativeTime(now - 60, now)).toBe('il y a 1 min');
    expect(relativeTime(now - 3599, now)).toBe('il y a 59 min');
    expect(relativeTime(now - 3600, now)).toBe('il y a 1 h');
    expect(relativeTime(now - 47 * 3600, now)).toBe('il y a 47 h');
    expect(relativeTime(now - 48 * 3600, now)).toBe('il y a 2 j');
  });

  it('treats a date in the future as now', () => {
    expect(relativeTime(now + 30, now)).toBe("à l'instant");
  });
});

describe('errorMessage', () => {
  it('translates a known server code', () => {
    const error = new ApiError(401, 'bad_credentials', 'wrong name or password');
    expect(errorMessage(error)).toBe('Nom ou mot de passe incorrect.');
  });

  it('falls back for an unknown code or a foreign error', () => {
    expect(errorMessage(new ApiError(500, 'brand_new', 'x'))).toBe('Une erreur est survenue.');
    expect(errorMessage(new Error('boom'))).toBe('Une erreur est survenue.');
  });
});
