// Tous les textes visibles de l'interface sont dans `fr.json`.
// Un composant ne contient jamais de texte en dur : il appelle `t('cle')`.
import fr from './fr.json';

export type CleMessage = keyof typeof fr;

/** Remplace chaque `{nom}` du modèle par la valeur correspondante. */
export function formater(modele: string, valeurs: Record<string, string | number> = {}): string {
  return modele.replace(/\{(\w+)\}/g, (marque, nom: string) => {
    const valeur = valeurs[nom];
    return valeur === undefined ? marque : String(valeur);
  });
}

/** Texte français de la clé, avec ses valeurs. */
export function t(cle: CleMessage, valeurs?: Record<string, string | number>): string {
  return formater(fr[cle], valeurs);
}
