import { describe, expect, it } from 'vitest';

import fr from './fr.json';
import { formater, t } from './index';

describe('formater', () => {
  it('remplace les marques par leurs valeurs', () => {
    expect(formater('{n} fichiers pour {nom}', { n: 3, nom: 'Léa' })).toBe('3 fichiers pour Léa');
  });

  it('remplace une marque répétée', () => {
    expect(formater('{a} et {a}', { a: 'x' })).toBe('x et x');
  });

  it('laisse une marque sans valeur telle quelle', () => {
    expect(formater('reste {inconnu}')).toBe('reste {inconnu}');
  });

  it('garde une valeur nulle ou vide', () => {
    expect(formater('{n} fichier, {nom}.', { n: 0, nom: '' })).toBe('0 fichier, .');
  });
});

describe('t', () => {
  it('renvoie le texte français de la clé', () => {
    expect(t('app.nom')).toBe('Banc');
  });

  it("n'a aucun message vide", () => {
    for (const [cle, texte] of Object.entries(fr)) {
      expect(texte.trim(), cle).not.toBe('');
    }
  });
});
