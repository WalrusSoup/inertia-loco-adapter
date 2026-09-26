import { createInertiaApp } from '@inertiajs/react'
import { renderToString } from 'react-dom/server'

const pages = import.meta.glob('./pages/**/*.jsx', { eager: true })

globalThis.InertiaSsr = {
  render(page) {
    return createInertiaApp({
      page,
      render: renderToString,
      resolve: name => pages[`./pages/${name}.jsx`],
      setup: ({ App, props }) => <App {...props} />,
    })
  },
}
