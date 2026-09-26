import { Head, InfiniteScroll, Link } from '@inertiajs/react'

export default function Index({ episodes }) {
  return <>
    <Head title="Dragon Ball Episode List" />
    <main className="episode-page">
      <Link href="/" className="back-link">← CHARACTER DATABASE</Link>
      <p>LOCO × INERTIA / GUIDE 02</p>
      <h1>Dragon Ball<br />Episode List</h1>
      <p className="episode-intro">The Saiyan saga, one episode at a time. Keep scrolling to load the next set.</p>
      <InfiniteScroll
        data="episodes"
        className="episode-scroll"
        next={({ loading, hasMore, fetch, autoMode }) =>
          !autoMode && hasMore ? <button className="load-more" onClick={fetch} disabled={loading}>{loading ? 'Loading…' : 'Load more episodes'}</button> : null
        }
        loading={({ loadingNext }) => loadingNext ? <p className="scroll-status">Loading episodes…</p> : null}
      >
        <table className="episode-table">
          <thead>
            <tr><th scope="col">Episode</th><th scope="col">Title</th><th scope="col">Runtime</th></tr>
          </thead>
          <tbody>
            {episodes.data.map(episode => <tr key={episode.number}>
              <td className="episode-number">{String(episode.number).padStart(3, '0')}</td>
              <td className="episode-title">{episode.title}</td>
              <td className="episode-runtime">{episode.runtime}</td>
            </tr>)}
          </tbody>
        </table>
      </InfiniteScroll>
      <p className="episode-count">{episodes.meta.total_items} episodes in this guide · loaded as you scroll</p>
    </main>
  </>
}
