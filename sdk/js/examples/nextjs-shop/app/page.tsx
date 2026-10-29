const ITEM = { name: "Parcel to Ikeja", amount_sats: 25_000 };

export default function Page() {
  return (
    <main>
      <h1>Example shop</h1>
      <p>
        <strong>{ITEM.name}</strong> — {ITEM.amount_sats.toLocaleString()} sats
      </p>
      <p>You pay on toshi-way. The money is locked, not sent, until you confirm delivery.</p>
      {/* A plain form: no JavaScript needed, and the API key never reaches the browser. */}
      <form action="/api/checkout" method="post">
        <button type="submit" style={{ padding: "0.6rem 1rem", fontSize: "1rem" }}>
          Pay with toshi-way
        </button>
      </form>
    </main>
  );
}
