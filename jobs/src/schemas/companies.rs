use entity::jobs_companies;
use poem_openapi::Object;
use uuid::Uuid;

#[derive(Object)]
pub struct Company {
    /// The unique identifier of the company
    pub id: Uuid,
    /// The name of the company
    pub name: String,
    /// The description of the company
    pub description: Option<String>,
    /// The website of the company
    pub website: Option<String>,
    /// A link to a YouTube video of the company
    pub youtube_video: Option<String>,
    /// The Twitter handle of the company
    pub twitter_handle: Option<String>,
    /// The Instagram handle of the company
    pub instagram_handle: Option<String>,
    /// The logo of the company
    pub logo_url: Option<String>,
}

impl From<jobs_companies::Model> for Company {
    fn from(
        jobs_companies::Model {
            id,
            name,
            description,
            website,
            youtube_video,
            twitter_handle,
            instagram_handle,
            logo_url,
        }: jobs_companies::Model,
    ) -> Self {
        Self {
            id,
            name,
            description,
            website,
            youtube_video,
            twitter_handle,
            instagram_handle,
            logo_url,
        }
    }
}

#[derive(Object)]
pub struct CreateCompany {
    /// The name of the company
    #[oai(validator(max_length = 255))]
    pub name: String,
    /// The description of the company
    #[oai(validator(max_length = 255))]
    pub description: Option<String>,
    /// The website of the company
    #[oai(validator(max_length = 255))]
    pub website: Option<String>,
    /// A link to a YouTube video of the company
    #[oai(validator(max_length = 255))]
    pub youtube_video: Option<String>,
    /// The Twitter handle of the company
    #[oai(validator(max_length = 255))]
    pub twitter_handle: Option<String>,
    /// The Instagram handle of the company
    #[oai(validator(max_length = 255))]
    pub instagram_handle: Option<String>,
    /// The logo of the company
    #[oai(validator(max_length = 255))]
    pub logo_url: Option<String>,
}

#[derive(Object)]
pub struct UpdateCompany {
    /// The name of the company
    #[oai(validator(max_length = 255))]
    pub name: Option<String>,
    /// The description of the company
    #[oai(validator(max_length = 255))]
    pub description: Option<Option<String>>,
    /// The website of the company
    #[oai(validator(max_length = 255))]
    pub website: Option<Option<String>>,
    /// A link to a YouTube video of the company
    #[oai(validator(max_length = 255))]
    pub youtube_video: Option<Option<String>>,
    /// The Twitter handle of the company
    #[oai(validator(max_length = 255))]
    pub twitter_handle: Option<Option<String>>,
    /// The Instagram handle of the company
    #[oai(validator(max_length = 255))]
    pub instagram_handle: Option<Option<String>>,
    /// The logo of the company
    #[oai(validator(max_length = 255))]
    pub logo_url: Option<Option<String>>,
}
