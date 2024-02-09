
use std::time::Duration;

use poise::CreateReply;
use serenity::all::{ButtonStyle, ComponentInteractionDataKind};
use serenity::builder::{CreateActionRow, CreateButton, CreateInteractionResponse, CreateMessage, CreateSelectMenu, CreateSelectMenuKind, CreateSelectMenuOption};
use serenity::prelude::*;


type Error = Box<dyn std::error::Error + Send + Sync>;
type Context<'a> = poise::Context<'a, Data, Error>;
// User data, which is stored and accessible in all command invocations
pub struct Data {
    database: Database,
}


#[tokio::main]
async fn main() {

    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            commands: vec![
                foundsomething(),
                needsomething(),
                whatdoineed(),
                dontneed(),
                help(),
                forgetme(),
            ],
            on_error: |_error| {
                Box::pin(async move {
                    println!("got an error");
                })
            },
            ..Default::default()
        })
        .setup(move |ctx, _ready, framework| {
            Box::pin(async move {
                poise::builtins::register_globally(ctx, &framework.options().commands).await?;
                Ok(Data {
                    database: Database::new(),
                })
            })
        })
        .build();


    let token = std::fs::read_to_string("token.txt").expect("could not read token in token.txt");

    let intents = GatewayIntents::non_privileged() | GatewayIntents::MESSAGE_CONTENT;

    let mut client = Client::builder(token, intents)
        // .event_handler(Handler)
        .framework(framework)
        .await
        .expect("Error creating client");

    println!("logged in");

    // start listening for events by starting a single shard
    if let Err(why) = client.start().await {
        println!("An error occurred while running the client: {:?}", why);
    }
}



const CLAIM_TIMEOUT: Duration = Duration::from_secs(60 * 3);

#[poise::command(slash_command)]
async fn foundsomething(
    ctx: Context<'_>,
) -> Result<(), Error> {

    let author_id = ctx.author().id;

    // this is the main reply message, visible to everyone and gets updated with the status of the share
    let status_reply = ctx.send(
        CreateReply::default().content(format!("<@{}> has found a cosmetic", author_id))
    ).await.unwrap();


    // get the cosmetic
    let Some(cosmetic) = cosmetic_select(ctx).await else {
        return Ok(());
    };


    // update the status with the cosmetic
    status_reply.edit(ctx, CreateReply::default().content(format!("<@{}> has found **{}**", author_id, cosmetic))).await.unwrap();


    // send a message to ping the users that need the cosmetic
    let needed_users = ctx.data().database.who_needs(&cosmetic);

    let content = if needed_users.len() == 0 {
        format!("<@{}> has found **{}** but no one needs it, you can still claim it if you need it\n\n", author_id, cosmetic)
    } else {

        let needed_users = needed_users.iter()
        .fold(String::new(), |acc, user| format!("{} <@{}>", acc, user));

        format!("<@{}> has found **{}**\n\n{}\n\nYou can still claim it if you weren't pinged, and if you have it but got pinged click \"Already Have It\"", author_id, cosmetic, needed_users)
    };

    let claim_reply = ctx.send(
        CreateReply::default()
            .content(content)
            .components(vec![
                CreateActionRow::Buttons(vec![
                    CreateButton::new("claim").style(ButtonStyle::Success).label("Claim"),
                    CreateButton::new("cancel").style(ButtonStyle::Danger).label("Cancel"),
                    CreateButton::new("have").style(ButtonStyle::Primary).label("Already Have It"),
                ]),
            ])
    ).await.unwrap();


    // wait for and parse the claim response
    let message = claim_reply.message().await.unwrap();

    let claimed_user = loop {
        if let Some(interaction) = message.await_component_interaction(&ctx.serenity_context().shard).timeout(CLAIM_TIMEOUT).await {
            interaction.create_response(ctx, CreateInteractionResponse::Acknowledge).await.unwrap();

            let ComponentInteractionDataKind::Button = interaction.data.kind else {
                println!("malformed component response. expected a `Button`, got {:?}", interaction.data.kind);
                return Ok(());
            };

            let id = interaction.data.custom_id.as_str();

            match id {
                "cancel" => {
                    if &interaction.user == ctx.author() {
                        // cancel the share
                        claim_reply.delete(ctx).await.unwrap();
                        status_reply.edit(ctx, CreateReply::default().content(format!("<@{}> found **{}** but cancelled", author_id, cosmetic))).await.unwrap();
                        return Ok(());
                    } else {
                        // dm the unauthorized user
                        interaction.create_response(ctx, CreateInteractionResponse::Acknowledge).await.unwrap();
                        interaction.user.dm(ctx, CreateMessage::new().content("Only the creator of the cosmetic share can cancel it")).await.unwrap();
                    }
                },
                "claim" => {
                    break interaction.user;
                },
                "have" => {
                    ctx.data().database.remove(&cosmetic, &interaction.user.id.to_string());
                },
                _ => {
                    println!("malformed component response. invalid button id \"{}\"", id);
                    return Ok(());
                }
            }
        } else {
            claim_reply.delete(ctx).await.unwrap();
            status_reply.edit(ctx, CreateReply::default().content(format!("<@{}> found **{}** but no one responded within {:#?}", author_id, cosmetic, CLAIM_TIMEOUT))).await.unwrap();
            return Ok(());
        }
    };


    // update the status and ping the user that they have claimed it
    claim_reply.delete(ctx).await.unwrap();
    status_reply.edit(ctx, CreateReply::default().content(format!("<@{}> found **{}** which was been claimed by <@{}>\n\nMake sure to use the `/dontneed` command later so you don't get pinged again", author_id, cosmetic, claimed_user.id))).await.unwrap();


    Ok(())
}



#[poise::command(slash_command)]
async fn needsomething(
    ctx: Context<'_>,
) -> Result<(), Error> {

    let status_reply = ctx.send(
        CreateReply::default()
            .content(format!("Select the cosmetic you need"))
            .ephemeral(true)
    ).await.unwrap();


    // get the cosmetic
    let Some(cosmetic) = cosmetic_select(ctx).await else {
        return Ok(());
    };


    if ctx.data().database.who_needs(&cosmetic).contains(&ctx.author().id.to_string()) {
        status_reply.edit(ctx, CreateReply::default().content("you already need that cosmetic")).await.unwrap();
    }


    // add to database
    ctx.data().database.add(&cosmetic, &ctx.author().id.to_string());
    status_reply.edit(ctx, CreateReply::default().content(format!("you now need **{}**", cosmetic))).await.unwrap();


    Ok(())
}



#[poise::command(slash_command)]
async fn whatdoineed(
    ctx: Context<'_>,
) -> Result<(), Error> {


    let cosmetics = ctx.data().database.needed_by(&ctx.author().id.to_string());

    let content = if cosmetics.len() > 0 {
        let content = cosmetics.iter().fold(String::new(), |acc, user| format!("{}\n**{}**", acc, user));

        format!("You need\n{}", content)
    } else {
        "You don't need anything".to_string()
    };

    ctx.send(
        CreateReply::default()
            .content(content)
            .ephemeral(true)
    ).await.unwrap();


    Ok(())
}



#[poise::command(slash_command)]
async fn dontneed(
    ctx: Context<'_>,
) -> Result<(), Error> {


    let status_reply = ctx.send(
        CreateReply::default()
            .content(format!("Select the cosmetic you don't need"))
            .ephemeral(true)
    ).await.unwrap();


    // get the cosmetic
    let Some(cosmetic) = cosmetic_select(ctx).await else {
        return Ok(());
    };


    let user_id = &ctx.author().id.to_string();

    if ctx.data().database.needs(user_id, &cosmetic) {
        ctx.data().database.remove(&cosmetic, user_id);

        status_reply.edit(ctx, CreateReply::default().content(format!("You now don't need **{}**", cosmetic))).await.unwrap();
    } else {
        status_reply.edit(ctx, CreateReply::default().content(format!("You already didn't need **{}**", cosmetic))).await.unwrap();
    }


    Ok(())
}



const HELP_MESSAGE: &str = "This is a discord bot for sharing cosmetics with the community.

You can tell it what cosmetics you need with `/needsomething`, and when you or someone finds a duplicate they can use the `/foundsomething` command to ping everyone that needs it.
Use `/whatdoineed` to see what the bot thinks you need and `/dontneed` to tell it what you've unlocked.

The bot keeps a shared database across all the servers it's in, but be aware that this means that users you don't share a server with might see your user.";

#[poise::command(slash_command)]
async fn help(
    ctx: Context<'_>,
) -> Result<(), Error> {


    ctx.send(
        CreateReply::default()
            .content(HELP_MESSAGE)
            .ephemeral(true)
    ).await.unwrap();


    Ok(())
}


#[poise::command(slash_command)]
async fn forgetme(
    ctx: Context<'_>,
) -> Result<(), Error> {


    let reply = ctx.send(
        CreateReply::default()
            .content("This will make the bot forget all the cosmetics you need and remove you from it's database in *all* servers. This is **irreversible**, if you've spent lots of time entering in cosmetics you'll lose that progress.")
            .ephemeral(true)
            .components(vec![
                CreateActionRow::Buttons(vec![
                    CreateButton::new("yes").style(ButtonStyle::Danger).label("Yes, Do It"),
                    CreateButton::new("no").style(ButtonStyle::Primary).label("Yeah... nevermind"),
                ])
            ]),
    ).await.unwrap();


    // wait for button press
    let message = reply.message().await.unwrap();

    let interaction = match message.await_component_interaction(&ctx.serenity_context().shard).timeout(Duration::from_secs(60)).await {
        Some(interaction) => interaction,
        None => {
            reply.delete(ctx).await.unwrap();
            ctx.send(CreateReply::default().ephemeral(true).content("Timed out")).await.unwrap();
            return Ok(());
        },
    };

    interaction.create_response(ctx, CreateInteractionResponse::Acknowledge).await.unwrap();

    let ComponentInteractionDataKind::Button = interaction.data.kind else {
        println!("malformed component response. expected a `Button`, got {:?}", interaction.data.kind);
        return Ok(());
    };

    let id = interaction.data.custom_id.as_str();

    match id {
        "yes" => {
            ctx.data().database.forget(&interaction.user.id.to_string());

            ctx.send(
                CreateReply::default()
                    .content("You've been deleted")
                    .ephemeral(true)
            ).await.unwrap();
        },
        "no" => {
            ctx.send(
                CreateReply::default()
                    .content("Cancelled")
                    .ephemeral(true)
            ).await.unwrap();
        }
        _ => {
            println!("malformed component response. invalid button id \"{}\"", id);
            return Ok(());
        }
    }


    Ok(())
}


async fn cosmetic_select(
    ctx: Context<'_>,
) -> Option<String> {


    // prompt to the author for the category of the cosmetic
    let category_reply = ctx.send(
        CreateReply::default()
        .content("Select category")
        .ephemeral(true)
        .components(vec![
            CreateActionRow::SelectMenu(
                CreateSelectMenu::new(
                    "category",
                    CreateSelectMenuKind::String {
                        options: Vec::from_iter(CosmeticCategory::ALL.into_iter().map(
                            |c| CreateSelectMenuOption::new(format!("{}", c), c.to_id())
                        ))
                    }
                )
            ),
        ])
    ).await.unwrap();


    // wait for and parse the response from the prompt
    let message = category_reply.message().await.unwrap();

    let interaction = match message.await_component_interaction(&ctx.serenity_context().shard).timeout(Duration::from_secs(60)).await {
        Some(interaction) => interaction,
        None => {
            category_reply.delete(ctx).await.unwrap();
            ctx.send(CreateReply::default().ephemeral(true).content("Timed out")).await.unwrap();
            return None;
        },
    };

    let ComponentInteractionDataKind::StringSelect { values } = &interaction.data.kind else {
        println!("malformed component response. expected a `StringSelect`, got {:?}", interaction.data.kind);
        return None;
    };

    let Some(id) = values.first() else {
        println!("malformed component response, there was no selected value");
        return None;
    };

    let Some(category) = CosmeticCategory::from_id(id) else {
        println!("malformed component response, invalid cosmetic category id \"{}\"", id);
        return None;
    };


    // acknowledge and delete the prompt
    interaction.create_response(ctx, CreateInteractionResponse::Acknowledge).await.unwrap();
    category_reply.delete(ctx).await.unwrap();


    // can only have up to 25 in each list, so break into chunks of 25
    let mut cosmetics = ctx.data().database.cosmetics_in_category(category);
    let mut chunks = Vec::new();

    loop {
        let chunk = Vec::from_iter((&mut cosmetics).take(25));
        if chunk.len() == 0 {
            break;
        }
        chunks.push(chunk);
    }

    // can only have 4 chunks at a time so break into pages
    let mut chunks = chunks.into_iter();
    let mut pages = Vec::new();

    loop {
        let page = Vec::from_iter((&mut chunks).take(4));
        if page.len() == 0 {
            break;
        }
        pages.push(page);
    }


    let create_page = |n: usize| -> CreateReply {

        let mut components = Vec::from_iter(
            pages.get(n).unwrap().iter().enumerate().map(|(i, chunk)| {
                CreateActionRow::SelectMenu(CreateSelectMenu::new(i.to_string(), CreateSelectMenuKind::String {
                    options: Vec::from_iter(chunk.into_iter().map(|&cosmetic| {
                        CreateSelectMenuOption::new(cosmetic, cosmetic)
                    }))
                }).placeholder(*chunk.first().unwrap()))
            })
        );

        components.push(CreateActionRow::Buttons(vec![
            CreateButton::new("back").label("< Page"),
            CreateButton::new("next").label("Page >"),
        ]));

        CreateReply::default()
        .content(format!("Select cosmetic\nPage **{}**", n + 1))
        .ephemeral(true)
        .components(components)
    };


    let mut current_page = 0;

    // prompt the author for the cosmetic in the category
    let cosmetic_reply = ctx.send(create_page(current_page)).await.unwrap();

    // wait for and parse the response from the prompt
    let message = cosmetic_reply.message().await.unwrap();

    let cosmetic = loop {
        let interaction = match message.await_component_interaction(&ctx.serenity_context().shard).timeout(Duration::from_secs(60)).await {
            Some(interaction) => interaction,
            None => {
                cosmetic_reply.delete(ctx).await.unwrap();
                ctx.send(CreateReply::default().ephemeral(true).content("Timed out")).await.unwrap();
                return None;
            },
        };

        interaction.create_response(ctx, CreateInteractionResponse::Acknowledge).await.unwrap();

        match interaction.data.kind {
            ComponentInteractionDataKind::StringSelect { values } => {
                let Some(cosmetic) = values.first() else {
                    println!("malformed component response, there was no selected value");
                    return None;
                };

                break cosmetic.clone();
            },

            ComponentInteractionDataKind::Button => {
                match interaction.data.custom_id.as_str() {
                    "next" => {
                        if current_page >= pages.len() - 1 {
                            current_page = 0;
                        } else {
                            current_page += 1;
                        }

                        cosmetic_reply.edit(ctx, create_page(current_page)).await.unwrap();
                    },
                    "back" => {
                        if current_page == 0 {
                            current_page = pages.len() - 1;
                        } else {
                            current_page -= 1;
                        }

                        cosmetic_reply.edit(ctx, create_page(current_page)).await.unwrap();
                    },
                    _ => {
                        println!("malformed component response");
                        return None;
                    }
                }
            },

            _ => {
                println!("malformed component response");
                return None;
            }
        }
    };


    cosmetic_reply.delete(ctx).await.unwrap();


    Some(cosmetic.clone())
}



#[derive(Clone, Copy)]
pub enum CosmeticCategory {
    Hat,
    Top,
    Bottom,
    Accessory,
    Vest,
    Belt,
}

impl CosmeticCategory {
    pub const ALL: &'static [CosmeticCategory] = &[
        Self::Hat,
        Self::Top,
        Self::Bottom,
        Self::Accessory,
        Self::Vest,
        Self::Belt,
    ];
}

impl std::fmt::Display for CosmeticCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Hat => "Hat",
            Self::Top => "Top",
            Self::Bottom => "Bottom",
            Self::Accessory => "Accessory",
            Self::Vest => "Vest",
            Self::Belt => "Belt",
        })
    }
}

impl CosmeticCategory {
    pub fn to_id(&self) -> &'static str {
        match self {
            Self::Hat => "0",
            Self::Top => "1",
            Self::Bottom => "2",
            Self::Accessory => "3",
            Self::Vest => "4",
            Self::Belt => "5",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "0" => Some(Self::Hat),
            "1" => Some(Self::Top),
            "2" => Some(Self::Bottom),
            "3" => Some(Self::Accessory),
            "4" => Some(Self::Vest),
            "5" => Some(Self::Belt),
            _ => None,
        }
    }
}



pub struct Database {
    hats: Vec<String>,
    tops: Vec<String>,
    bottoms: Vec<String>,
    accessories: Vec<String>,
    vests: Vec<String>,
    belts: Vec<String>,

    all: Vec<String>,
}


impl Database {
    pub fn new() -> Database {

        let mut database = Database {
            hats: Vec::new(),
            tops: Vec::new(),
            bottoms: Vec::new(),
            accessories: Vec::new(),
            vests: Vec::new(),
            belts: Vec::new(),

            all: Vec::new(),
        };

        for category in CosmeticCategory::ALL {

            let path = format!("cosmetics/{}.txt", category);

            let cosmetics = std::fs::read_to_string(&path).expect(&format!("could not read {:?}", path));

            let cosmetics = filter_allowed_characters(cosmetics);

            let mut cosmetics = Vec::from_iter(
                cosmetics.split("\n")
                .map(str::to_string)
                .filter(|line| line.len() > 0)
            );

            for cosmetic in cosmetics.iter() {
                database.all.push(cosmetic.clone());
            }

            cosmetics.sort();

            *match category {
                CosmeticCategory::Hat => &mut database.hats,
                CosmeticCategory::Top => &mut database.tops,
                CosmeticCategory::Bottom => &mut database.bottoms,
                CosmeticCategory::Accessory => &mut database.accessories,
                CosmeticCategory::Vest => &mut database.vests,
                CosmeticCategory::Belt => &mut database.belts,
            } = cosmetics;
        }

        database
    }

    pub fn cosmetics_in_category(&self, category: CosmeticCategory) -> impl Iterator<Item = &String> + '_ {
        match category {
            CosmeticCategory::Hat => self.hats.iter(),
            CosmeticCategory::Top => self.tops.iter(),
            CosmeticCategory::Bottom => self.bottoms.iter(),
            CosmeticCategory::Accessory => self.accessories.iter(),
            CosmeticCategory::Vest => self.vests.iter(),
            CosmeticCategory::Belt => self.belts.iter(),
        }
    }

    pub fn needs(&self, user_id: &str, cosmetic: &str) -> bool {
        self.who_needs(cosmetic).iter().any(|line| line == user_id)
    }

    pub fn who_needs(&self, cosmetic: &str) -> Vec<String> {
        if let Ok(users) = std::fs::read_to_string(format!("database/{}.txt", cosmetic)) {

            let users = filter_allowed_characters(users);

            Vec::from_iter(
                users.split("\n")
                .map(str::to_string)
                .filter(|line| line.len() > 0)
            )
        } else {
            Vec::new()
        }
    }

    pub fn add(&self, cosmetic: &str, user_id: &str) {
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(format!("database/{}.txt", cosmetic))
            .expect(&format!("could not open file for cosmetic {}", cosmetic));

        use std::io::Write;
        writeln!(file, "{}", user_id).expect(&format!("could not write to file for cosmetic {}", cosmetic));
    }

    /// currently naive approach
    pub fn needed_by(&self, user_id: &str) -> Vec<String> {
        let mut needed = Vec::new();

        for cosmetic in self.all.iter() {
            if self.needs(user_id, cosmetic) {
                needed.push(cosmetic.clone());
            }
        }

        needed
    }

    pub fn remove(&self, cosmetic: &str, user_id: &str) {

        let path = format!("database/{}.txt", cosmetic);

        let Ok(users) = std::fs::read_to_string(&path) else {
            return;
        };

        let users = filter_allowed_characters(users);

        let users = String::from_iter(
            users.split('\n')
            .filter(|&line| line != user_id)
        );

        std::fs::write(&path, users).unwrap();
    }

    pub fn forget(&self, user_id: &str) {

        for cosmetic in self.all.iter() {
            self.remove(cosmetic, user_id);
        }
    }
}


fn filter_allowed_characters(string: String) -> String {
    String::from_iter(
        string.chars()
        .filter(|&c| {
            c.is_ascii_alphanumeric()
            || c == ' '
            || c == '\n'
            || c == '\''
            || c == '\"'
            || c == '-'
            || c == '#'
            || c == '('
            || c == ')'
        }
    ))
}
