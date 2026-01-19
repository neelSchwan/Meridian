use std::io::stdout;

use riven::RiotApi;
use riven::consts::PlatformRoute;
use dotenv::dotenv;

fn main() {
    dotenv().ok();
    let api_key = std::env::var("RGAPI_KEY")
            .expect("Missing RGAPI_KEY in environment (.env or shell)");

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let riot_api = RiotApi::new(api_key);

        let na1 = PlatformRoute::NA1;
        let euw = PlatformRoute::EUW1;

        let account = riot_api.account_v1()
            .get_by_riot_id(na1.to_regional(), "diamondshovel678", "123").await
            .expect("get summoner failed")
            .expect("there isn't a player with that name");

        // get most recent match
        // get puuids from most recent match (check for dupes at this)
        // store in sum

        let match_ids = riot_api.match_v5()
            .get_match_ids_by_puuid(na1.to_regional(), &account.puuid, None, None, Some(riven::consts::Queue::SUMMONERS_RIFT_5V5_RANKED_SOLO), None, None, None).await
            .expect("can't get match id");

        for id in match_ids {
            let m = riot_api.match_v5().get_match(na1.to_regional(), &id).await
                .expect("get match failed")
                .expect("there isn't a match with that id");

            for player in m.metadata.participants {
                let player = riot_api.account_v1().get_by_puuid(na1.to_regional(), &player).await
                    .expect("getting summoner failed");

                println!("{:?}", player.game_name);
            }
        }
    })
}
