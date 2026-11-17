# HITL host

On a Mac running the HITL host, `hitl check` is the window for questions, plans, results and actions. The host also serves the same queue to a paired iPhone. See `phone.md` for checking an iPhone build.

The result is the person's decision about the checked work. Handle subsequent repository actions using the rules of that repository.

For a manual iOS app check running on this Mac, use `hitl mac [PROJECT] [--scheme NAME] -- --title TEXT --about TEXT --check TEXT …`. Before `--` are build options, after it the ordinary HITL card. It uses `ios-run --mac --run`, shares the slot with Mac tests, and holds it until the card is finished or dropped. Run the whole command as a background job with a log if the chat should stay free. Do not launch the candidate separately.

Optional T3 delivery: run `hitl t3-connect` and open its one-time OAuth link on the Mac. Approve an access level that allows sending messages. HITL uses T3's local MCP endpoint with `mode: auto` for checks from that T3 environment. Busy turns that accept steering get the reply in the current turn; other turns follow T3's queue rules. Without a valid connection HITL uses the existing CLI queue. If T3 does not confirm a send already attempted over MCP, HITL preserves the answer for `hitl wait` without sending a second copy. Revoke HITL in T3's Sharing & access settings; run `hitl t3-connect` again to reconnect when the grant expires.
