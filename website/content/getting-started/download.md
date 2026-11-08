# Download your games

Fast can fetch your games from Lichess or Chess.com into a database. It only connects when you
click **Download games** or **Update**, and it sends nothing but the username and how far back
to look.

## Download

1. Open **Database › Import** and choose **Lichess** or **Chess.com** as the source.
2. Type the username, and choose how far back to go: **All**, **Last year**, **Last month** or
   **Since last update**.
3. Choose where the games go. My games is the default, so the My games column and leak
   detection pick them up at once.
4. Click **Download games**.

The dialog shows the games so far and a progress bar. Close it to keep the download running in
the background; the Database screen shows its progress and a **Cancel** button. Cancelling
keeps every game fetched so far.

- **Include clocks and evaluations** (Lichess) keeps clock times and Lichess's engine
  evaluations as comments, which Analyse reads.
- **This is my account** adds the username to your names, so games under it count as yours
  everywhere your player name does.
- Games already in the database are skipped and counted, so downloading again never makes
  duplicates.

## Update

Fast remembers each account. **Settings › Databases** lists them with the date of the last
update, an **Update** button and **Forget**. Right-clicking a database also offers
**Update <name> from Lichess** or **Update <name> from Chess.com**. An update fetches only the games played
since the last one. Updates never run on their own.

## When something goes wrong

| Message                               | What to do                                   |
| ------------------------------------- | -------------------------------------------- |
| No Lichess user named '…'             | Check the spelling of the username           |
| You're offline. Try again later.      | Check your connection                        |
| Lichess asks us to wait. Retrying in… | Fast waits a minute and tries once more      |
| The connection broke after … games    | Click **Update** to fetch the rest           |
