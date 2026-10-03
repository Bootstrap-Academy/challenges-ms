-- Entirely synthetic. Both ownership directions matter: user's subtasks in
-- another creator's task and other users' private subtasks in the user's task.
INSERT INTO challenges_challenge_categories (id, title, description, creation_timestamp)
VALUES ('00000000-0000-0000-0000-000000000009', 'Shared category', 'Category, no stored author', '2026-09-01');
INSERT INTO challenges_tasks (id, creator, creation_timestamp) VALUES
('00000000-0000-0000-0000-000000000001', '00000000-0000-0000-0000-000000000100', '2026-09-01'),
('00000000-0000-0000-0000-000000000002', '00000000-0000-0000-0000-000000000100', '2026-09-01'),
('00000000-0000-0000-0000-000000000003', '00000000-0000-0000-0000-000000000200', '2026-09-01');
INSERT INTO challenges_challenges (task_id, category_id, skill_ids, title, description) VALUES
('00000000-0000-0000-0000-000000000001', '00000000-0000-0000-0000-000000000009', ARRAY['python', 'unicode'], 'Authored title 雪', E'Authored parent\n```python\nprint("€")\n```');
INSERT INTO challenges_course_tasks (task_id, course_id, section_id, lecture_id) VALUES
('00000000-0000-0000-0000-000000000002', 'course', NULL, NULL),
('00000000-0000-0000-0000-000000000003', 'shared-course', 'section', 'lecture');
INSERT INTO challenges_subtasks (id, task_id, creator, creation_timestamp, xp, coins, enabled, retired, ty) VALUES
('00000000-0000-0000-0000-000000000101', '00000000-0000-0000-0000-000000000001', '00000000-0000-0000-0000-000000000100', '2026-09-01', 10, 2, false, true, 'coding_challenge'),
('00000000-0000-0000-0000-000000000102', '00000000-0000-0000-0000-000000000003', '00000000-0000-0000-0000-000000000100', '2026-09-01', 11, 3, false, false, 'question'),
('00000000-0000-0000-0000-000000000103', '00000000-0000-0000-0000-000000000002', '00000000-0000-0000-0000-000000000100', '2026-09-01', 12, 4, true, false, 'multiple_choice_question'),
('00000000-0000-0000-0000-000000000104', '00000000-0000-0000-0000-000000000002', '00000000-0000-0000-0000-000000000100', '2026-09-01', 13, 5, true, false, 'matching'),
('00000000-0000-0000-0000-000000000105', '00000000-0000-0000-0000-000000000001', '00000000-0000-0000-0000-000000000200', '2026-09-01', 14, 6, false, false, 'coding_challenge'),
('00000000-0000-0000-0000-000000000106', '00000000-0000-0000-0000-000000000001', '00000000-0000-0000-0000-000000000200', '2026-09-01', 15, 7, false, false, 'question'),
('00000000-0000-0000-0000-000000000107', '00000000-0000-0000-0000-000000000002', '00000000-0000-0000-0000-000000000100', '2026-09-01', 0, 0, false, true, 'coding_challenge');
INSERT INTO challenges_coding_challenges (subtask_id, time_limit, memory_limit, evaluator, description, solution_environment, solution_code, static_tests, random_tests) VALUES
('00000000-0000-0000-0000-000000000101', 1234, 256, 'replaced by parameterized source', 'Markdown with image data:image/png;base64,AP/+AQ==', 'python', 'replaced by parameterized source', 7, 13),
('00000000-0000-0000-0000-000000000105', 1000, 128, 'OTHER_AUTHOR_PRIVATE_EVALUATOR', 'Other description', 'python', 'OTHER_AUTHOR_PRIVATE_SOLUTION', 1, 1),
('00000000-0000-0000-0000-000000000107', 1000, 128, '', '', '', '', 0, 0);
INSERT INTO challenges_questions (subtask_id, question, answers, case_sensitive, ascii_letters, digits, punctuation, blocks) VALUES
('00000000-0000-0000-0000-000000000102', E'## Frage 雪\n\t"Was?"\r\n', ARRAY['Ä', '', 'Ä', E'line\nend'], true, false, true, false, ARRAY['```', '雪', '']),
('00000000-0000-0000-0000-000000000106', 'Other question', ARRAY['OTHER_AUTHOR_PRIVATE_ANSWER'], false, true, true, true, ARRAY[]::text[]);
INSERT INTO challenges_multiple_choice_quizes (subtask_id, question, answers, correct_answers, single_choice) VALUES
('00000000-0000-0000-0000-000000000103', 'Choose all', ARRAY(SELECT 'answer-' || n FROM generate_series(0, 63) n), -9223372036854775807, false);
INSERT INTO challenges_matchings (subtask_id, "left", "right", solution) VALUES
('00000000-0000-0000-0000-000000000104', ARRAY['甲', '乙', '丙'], ARRAY['C', 'A', 'B'], ARRAY[1, 2, 0]::smallint[]);
INSERT INTO challenges_coding_challenge_submissions (id, subtask_id, creator, creation_timestamp, environment, code) VALUES
('00000000-0000-0000-0000-000000000301', '00000000-0000-0000-0000-000000000105', '00000000-0000-0000-0000-000000000100', '2026-09-01', 'python', 'MY_SUBMISSION_TO_OTHER_TASK'),
('00000000-0000-0000-0000-000000000302', '00000000-0000-0000-0000-000000000101', '00000000-0000-0000-0000-000000000200', '2026-09-01', 'python', 'OTHER_AUTHOR_PRIVATE_SUBMISSION');
INSERT INTO challenges_question_attempts (id, question_id, user_id, timestamp, solved) VALUES
('00000000-0000-0000-0000-000000000303', '00000000-0000-0000-0000-000000000106', '00000000-0000-0000-0000-000000000100', '2026-09-01', true),
('00000000-0000-0000-0000-000000000304', '00000000-0000-0000-0000-000000000102', '00000000-0000-0000-0000-000000000200', '2026-09-01', false);
