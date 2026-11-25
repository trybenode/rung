# Who does what (Stellar Buildathon, Nov 25 to Dec 1)

Two people, one repo, separate folders. If you do not edit the same files, you do not get merge conflicts.

## Ownership

| Folder | Owner | What |
|---|---|---|
| `contracts/` | Allen | Escrow contract, tests, deploys. Interface in `docs/INTERFACE.md` is frozen |
| `web/` | Allen | Next.js app: Freighter wallet, create job, job page, freelancer page, dispute and support screens |
| `backend/` | Co-founder | API and database: profiles, tiers, evidence upload and SHA-256, support queue, signs the arbiter ruling |
| `agent/` | Co-founder | Arbiter agent: reads both sides, recommends a ruling (shares logic with the Agentware project) |
| `docs/API.md` | Co-founder | Backend and agent endpoints. Change it freely, tell Allen |
| `deployments/` | Allen | Contract ID and public addresses. Everyone reads it |

Swap anything that does not match your strengths. Just say so in the group before starting.

## Schedule

The Agentware hackathon runs Nov 27 to 30, so the Stellar work is front-loaded and the middle days stay small.

**Wed Nov 25 (today)**
- Allen: push this repo, invite the co-founder, build and deploy the Day 2 contract (`docs/DEMO_CLI.md`), fill `deployments/testnet.json`, run the dispute flow from the terminal once. Start the Next.js app and Freighter connect.
- Co-founder: clone, read `docs/INTERFACE.md` and `docs/API.md`. Backend skeleton: database tables, `/health`, `/freelancers/:address`, evidence upload that returns a SHA-256. Agent first cut against the JSON shape in `docs/API.md`.

**Thu Nov 26**
- Allen: create-job screen (reads the tier, sets the terms), job page (milestones, approve, dispute), freelancer actions (accept, submit).
- Co-founder: tier rules and the admin verify endpoint, `/disputes/:id/:index/analyze`, the support queue, and `/rule` which signs `resolve_dispute` with the arbiter key. A small keeper script that calls `claim_after_window` and `arbiter_timeout` when due is a bonus.

**Fri Nov 27 to Mon Nov 30 (Agentware days)**: integration only, about two hours a day each.
- Allen: connect the app to the backend (evidence upload, then hash, then `submit_milestone`), dispute screen, support screen.
- Co-founder: fix backend bugs, tune the agent prompt, prepare demo data.

**Mon Nov 30 evening**: feature freeze. Record the demo video. **Tue Dec 1**: submit by the afternoon, not at midnight.

## If time runs short, cut in this order
1. Polish and styling.
2. The live agent (use a pre-written recommendation).
3. Tier logic (show fixed badges).
4. Evidence upload (paste a hash instead).

Never cut: the contract, the main flow, and the dispute flow working live.

## Git rules
- `main` always runs. Work on short branches: `allen/<thing>` and `cf/<thing>`.
- Small commits. `git pull --rebase` before every push.
- Stay in your own folders. For anything in the other person's folder, ask first.
- Contract changes: only Allen. He updates `docs/INTERFACE.md`, redeploys, updates `deployments/testnet.json`, and posts the new contract ID in the group in the same hour.
- Never commit `.env`, secret keys, `target/` or `node_modules/`. Copy `.env.example` files and fill them locally.

## Keys
Testnet only. Each person can make their own identities with `stellar keys generate`. The backend needs the arbiter
secret (carol). Allen sends it privately, never in git or in the group chat.

## Handoffs
- Allen to co-founder: contract ID, the `deployments/testnet.json` file, and the generated TypeScript bindings if needed.
- Co-founder to Allen: backend running locally or deployed, base URL in `web/.env.example`, endpoints that match `docs/API.md`.
