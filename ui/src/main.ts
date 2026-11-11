import { mount } from 'svelte';

import App from './App.svelte';

const target = document.getElementById('app');
if (!target) {
  throw new Error('élément #app absent de index.html');
}

export default mount(App, { target });
