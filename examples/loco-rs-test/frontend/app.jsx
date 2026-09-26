import { createInertiaApp } from '@inertiajs/react'
import { createRoot, hydrateRoot } from 'react-dom/client'
import './style.css'

const pages = import.meta.glob('./pages/**/*.jsx', { eager: true })
createInertiaApp({
  resolve: name => pages[`./pages/${name}.jsx`],
  setup({ el, App, props }) {
    if (el.hasChildNodes()) {
      hydrateRoot(el, <App {...props} />)
    } else {
      createRoot(el).render(<App {...props} />)
    }
  },
})

