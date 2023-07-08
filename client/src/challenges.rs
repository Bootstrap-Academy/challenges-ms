use schemas::challenges::{
    challenges::{
        Category, Challenge, CreateCategoryRequest, CreateChallengeError, CreateChallengeRequest,
        DeleteCategoryError, DeleteChallengeError, GetCategoryError, GetChallengeError,
        UpdateCategoryError, UpdateCategoryRequest, UpdateChallengeError, UpdateChallengeRequest,
    },
    subtasks::SubtasksUserConfig,
};

use super::client;

client!(Challenges {
    pub get_subtasks_user_config(): get "subtasks/user_config" => SubtasksUserConfig;

    pub list_categories(): get "categories" => Vec<Category>;
    pub get_category(path: category_id): get "categories/{category_id}" => Category, GetCategoryError;
    pub create_category(json: CreateCategoryRequest): post "categories" => Category;
    pub update_category(path: category_id, json: UpdateCategoryRequest): patch "categories/{category_id}" => Category, UpdateCategoryError;
    pub delete_category(path: category_id): delete "categories/{category_id}" => Success, DeleteCategoryError;
    pub list_challenges(path: category_id): get "categories/{category_id}/challenges" => Vec<Challenge>;
    pub get_challenge(path: category_id, path: challenge_id): get "categories/{category_id}/challenges/{challenge_id}" => Challenge, GetChallengeError;
    pub create_challenge(path: category_id, json: CreateChallengeRequest): post "categories/{category_id}/challenges" => Challenge, CreateChallengeError;
    pub update_challenge(path: category_id, path: challenge_id, json: UpdateChallengeRequest): patch "categories/{category_id}/challenges/{challenge_id}" => Challenge, UpdateChallengeError;
    pub delete_challenge(path: category_id, path: challenge_id): delete "categories/{category_id}/challenges/{challenge_id}" => Success, DeleteChallengeError;
});
