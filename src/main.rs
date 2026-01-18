use riven::RiotApi;
use riven::consts::PlatformRoute;
use dotenv::dotenv;
fn main() {
    dotenv().ok();

    let api_key = std::env::var("RGAPI_KEY")
            .expect("Missing RGAPI_KEY in environment (.env or shell)");

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        
        let riot_api = RiotApi::new(api_key); // unwrap ok because this never fails

        // region
        let platform = PlatformRoute::NA1;

        // get account
        let account = riot_api.account_v1()
            .get_by_riot_id(platform.to_regional(), "diamondshovel678", "123").await
            .expect("Get summoner failed")
            .expect("There is no summoner with that name.");

        let matches = riot_api.match_v5()
            .get_match_ids_by_puuid(platform.to_regional(), &account.puuid, Some(1), None, None, None, None, None).await
            .expect("Couldn't get matches for that puuid");

        for game in matches {
            let match_data = riot_api.match_v5().get_match(platform.to_regional(), &game).await
                .expect("error 1")
                .expect("error 2");

            println!("All match data: {:?}\n", match_data.metadata.participants.iter().collect::<Vec<_>>());
        }
    })
}
