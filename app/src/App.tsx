import { useState } from "react";

import { Account } from "./components/Account";
import { History } from "./components/History";
import { SidePanel } from "./components/SidePanel";
import { Stage } from "./components/Stage";
import { StatusBar } from "./components/StatusBar";
import { TopBar } from "./components/TopBar";
import { useMarket } from "./hooks/useMarket";

export default function App() {
  const { state, shares, pools, take } = useMarket();
  const [picked, setPicked] = useState<string | null>(null);

  const tip = state.blocks[0]?.height ?? null;
  let poolTotal = 0;
  for (const v of pools.values()) poolTotal += v;

  return (
    <div className="app">
      <TopBar height={tip} source={state.source} />

      <main className="body">
        <div className="main-col">
          <Stage
            blocks={state.blocks}
            elapsed={state.elapsed}
            poolTotal={poolTotal}
            picked={state.position?.miner ?? picked}
          />
          <History rounds={state.rounds} />
          <StatusBar
            shares={shares}
            counted={state.blocks.length}
            picked={state.position?.miner ?? picked}
          />
        </div>

        <aside className="rail">
          <SidePanel
            position={state.position}
            shares={shares}
            rounds={state.rounds}
            picked={picked}
            onPick={setPicked}
            onTake={take}
          />
          <Account
            balance={state.balance}
            session={state.session}
            hits={state.hits}
            staked={state.staked}
          />
        </aside>
      </main>
    </div>
  );
}
