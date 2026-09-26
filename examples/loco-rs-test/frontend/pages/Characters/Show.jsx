import { Head, Link } from '@inertiajs/react'
export default function Show({ character }) {
 const c = character.character
 return <><Head title={`${c.name} · DBZ Character Database`} /><main><Link href="/">← ALL CHARACTERS</Link><p>CHARACTER FILE / {String(c.id).padStart(2,'0')}</p><h1>{c.name}</h1><p>{c.description}</p><dl><dt>Species</dt><dd>{c.race}</dd><dt>Affiliation</dt><dd>{c.role}</dd><dt>Home world</dt><dd>{c.home_planet}</dd><dt>Power index</dt><dd>{c.power_level.toLocaleString()}</dd></dl><h2>Transformations</h2><ul>{character.transformations.map(x=><li key={x}>{x}</li>)}</ul><h2>Connected</h2><ul>{character.allies.map(x=><li key={x}>{x}</li>)}</ul></main></>
}
