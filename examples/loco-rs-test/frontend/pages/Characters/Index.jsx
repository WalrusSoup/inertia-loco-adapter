import { Head, Link } from '@inertiajs/react'
export default function Index({ characters }) {
  return <><Head title="Dragon Ball Z · Character Database" /><main><p>LOCO × INERTIA / DATABASE 01</p><h1>Earth’s defenders<br/>and its threats.</h1><p>A field guide to the people who shaped the Dragon Ball Z universe.</p><Link href="/episodes" className="episode-promo"><span>NOW PLAYING / GUIDE 02</span><b>Dragon Ball Episode List</b><span>Browse episodes <span aria-hidden="true">↗</span></span></Link><section>{characters.map((c, i) => <Link key={c.id} href={`/characters/${c.id}`} className="character"><span>{String(i + 1).padStart(2, '0')}</span><b>{c.name}</b><span>{c.race}</span><span>{c.role}</span><span>↗</span></Link>)}</section></main></>
}
