/** Player books panel — IDs are intent targets. */

import type { BooksView } from "../../../lib/parseBlackboxArena";

export interface BlackboxBooksPanelProps {
  books: BooksView;
}

function dash(n: number | null | undefined): string {
  return n === null || n === undefined ? "—" : String(n);
}

export function BlackboxBooksPanel({ books }: BlackboxBooksPanelProps) {
  return (
    <section className="bxs-books" aria-label="Firm books">
      <header className="bxs-panel-head">
        <span className="bxs-panel-title">BOOKS</span>
      </header>
      <div className="bxs-books-summary">
        <span>CASH {books.cashMinor} minor</span>
        <span>INV {books.inventoryValueMinor} minor</span>
        <span>SENIOR {books.seniorDebtMinor} minor</span>
        <span>MEZZ {books.mezzanineDebtMinor} minor</span>
      </div>

      <table className="bxs-table">
        <caption>SKU</caption>
        <thead>
          <tr>
            <th>SKU</th>
            <th>PRICE</th>
            <th>UNITS</th>
            <th>VALUE</th>
          </tr>
        </thead>
        <tbody>
          {books.skus.map((s) => (
            <tr key={s.sku}>
              <td>{s.sku}</td>
              <td>{s.unitPriceMinor}</td>
              <td>{s.inventoryUnits}</td>
              <td>{s.inventoryValueMinor}</td>
            </tr>
          ))}
        </tbody>
      </table>

      <table className="bxs-table">
        <caption>ACTIVE PROJECTS</caption>
        <thead>
          <tr>
            <th>ID</th>
            <th>COMMITTED</th>
            <th>CONTINUE#</th>
          </tr>
        </thead>
        <tbody>
          {books.projects.length === 0 ? (
            <tr>
              <td colSpan={3}>—</td>
            </tr>
          ) : (
            books.projects.map((p) => (
              <tr key={p.id}>
                <td>{p.id}</td>
                <td>{p.committedMinor}</td>
                <td>{p.continueCount}</td>
              </tr>
            ))
          )}
        </tbody>
      </table>

      <table className="bxs-table">
        <caption>OPEN OFFERS</caption>
        <thead>
          <tr>
            <th>ID</th>
            <th>KIND</th>
            <th>COST</th>
          </tr>
        </thead>
        <tbody>
          {books.offers.length === 0 ? (
            <tr>
              <td colSpan={3}>—</td>
            </tr>
          ) : (
            books.offers.map((o) => (
              <tr key={o.id}>
                <td>{o.id}</td>
                <td>{o.kind}</td>
                <td>{o.costMinor}</td>
              </tr>
            ))
          )}
        </tbody>
      </table>

      <table className="bxs-table">
        <caption>OPEN POSITIONS</caption>
        <thead>
          <tr>
            <th>ID</th>
            <th>INST</th>
            <th>NOTIONAL</th>
            <th>ENTRY</th>
          </tr>
        </thead>
        <tbody>
          {books.positions.length === 0 ? (
            <tr>
              <td colSpan={4}>—</td>
            </tr>
          ) : (
            books.positions.map((p) => (
              <tr key={p.id}>
                <td>{p.id}</td>
                <td>{p.instrument}</td>
                <td>{p.notionalMinor}</td>
                <td>{dash(p.entryIndexCenti)}</td>
              </tr>
            ))
          )}
        </tbody>
      </table>
    </section>
  );
}
