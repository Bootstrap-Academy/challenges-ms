use schemas::challenges::{
    challenges::{Category, CreateCategoryRequest, DeleteCategoryError},
    subtasks::SubtasksUserConfig,
};

use super::client;

client!(Challenges {
    pub get_subtasks_user_config(): get "subtasks/user_config" => SubtasksUserConfig;

    pub list_challenge_categories(): get "categories" => Vec<Category>;
    pub create_challenge_category(json: CreateCategoryRequest): post "categories" => Category;
    pub delete_challenge_category(path: category_id): delete "categories/{category_id}" => Success, DeleteCategoryError;
});
