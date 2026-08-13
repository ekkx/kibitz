import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { App } from './App.tsx';

// `index.css` is the Tailwind v4 + shadcn/ui entry: preflight, the utility
// layers, and the milk-coffee palette. `styles.css` is what the migration to
// shadcn could not express as utilities — the classification colours, the eval
// bar, the badge — and nothing else; its header lists the test each rule had to
// pass to stay.
//
// The order still matters, for one narrow reason. `styles.css` is *unlayered*,
// and unlayered rules beat anything inside `@layer`, which is where preflight
// and every Tailwind utility live. So its rules win ties regardless of source
// order, and importing it second only makes that visible rather than accidental.
// The two no longer share a single custom-property name, so there is nothing
// here for the cascade to arbitrate — see the palette note in `index.css`.
import './index.css';
import './styles.css';

const host = document.getElementById('root');
if (!host) throw new Error('#root is missing from index.html');

createRoot(host).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
