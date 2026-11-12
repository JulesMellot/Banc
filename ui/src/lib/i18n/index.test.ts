import { describe, expect, it } from 'vitest';

import fr from './fr.json';
import { format, t } from './index';

describe('format', () => {
  it('replaces placeholders with their values', () => {
    expect(format('{n} files for {name}', { n: 3, name: 'Lea' })).toBe('3 files for Lea');
  });

  it('replaces a repeated placeholder', () => {
    expect(format('{a} and {a}', { a: 'x' })).toBe('x and x');
  });

  it('leaves a placeholder without a value as is', () => {
    expect(format('left {unknown}')).toBe('left {unknown}');
  });

  it('keeps a zero or empty value', () => {
    expect(format('{n} file, {name}.', { n: 0, name: '' })).toBe('0 file, .');
  });
});

describe('t', () => {
  it('returns the French text for the key', () => {
    expect(t('app.name')).toBe('Banc');
  });

  it('has no empty message', () => {
    for (const [key, text] of Object.entries(fr)) {
      expect(text.trim(), key).not.toBe('');
    }
  });
});
