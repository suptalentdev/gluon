let { Option, Result, Ordering } = import "std/types.hs"
type List a = | Nil | Cons a (List a)
type Monoid m = {
    (<>): m -> m -> m,
    empty: m
}
let monoid_Function m: Monoid b -> (Monoid (a -> b)) = {
    (<>) = \f g -> \x -> m.(<>) (f x) (g x),
    empty = m.empty
}

let monoid_List =
    let (<>) xs ys =
        case xs of
            | Cons x zs -> Cons x (zs <> ys)
            | Nil -> ys
    { (<>), empty = Nil }

let monoid_Option m: Monoid a -> Monoid (Option a) = {
    (<>) = \l r ->
        case l of
            | Some x ->
                (case r of
                    | Some y -> Some (m.(<>) x y)
                    | None -> l)
            | None -> r,
    empty = None
}

let monoid_Int_Add = {
    (<>) = \x y -> x #Int+ y,
    empty = 0
}

let monoid_Int_Mul = {
    (<>) = \x y -> x #Int* y,
    empty = 1
}

let monoid_Float_Add = {
    (<>) = \x y -> x #Float+ y,
    empty = 0.0
}

let monoid_Float_Mul = {
    (<>) = \x y -> x #Float* y,
    empty = 1.0
}

let id x = x

let const x = \_ -> x

let flip f = \x y -> f y x

let not x = if x then False else True

let concatMap f xs: (a -> List b) -> List a -> List b =
    case xs of
        | Cons x ys -> monoid_List.(<>) (f x) (concatMap f ys)
        | Nil -> Nil

let foldl f x xs =
    case xs of
        | Cons y ys -> foldl f (f x y) ys
        | Nil -> x

let foldr f x xs =
    case xs of
        | Cons y ys -> f y (foldr f x ys)
        | Nil -> x

type Eq a = {
    (==) : a -> a -> Bool
}

let eq_Unit: Eq () = {
    (==) = \l r -> True
}

let eq_Int = {
    (==) = \l r -> l #Int== r
}

let eq_Float = {
    (==) = \l r -> l #Float== r
}

let eq_Option: Eq a -> Eq (Option a) = \eq_a -> {
    (==) = \l r ->
        case l of
            | Some l_val ->
                (case r of
                    | Some r_val -> eq_a.(==) l_val r_val
                    | None -> False)
            | None -> 
                (case r of
                    | Some _ -> False
                    | None -> True)
}

let eq_Result: Eq e -> Eq t -> Eq (Result e t) = \eq_e eq_t -> {
    (==) = \l r ->
        case l of
            | Ok l_val ->
                (case r of
                    | Ok r_val -> eq_t.(==) l_val r_val
                    | Err _ -> False)
            | Err l_val -> 
                (case r of
                    | Ok _ -> False
                    | Err r_val -> eq_e.(==) l_val r_val)
}

let eq_List: Eq a -> Eq (List a) = \d ->
    let (==) l r =
        case l of
            | Nil ->
                case r of
                    | Nil -> True
                    | Cons x y -> False
            | Cons x xs ->
                case r of
                    | Nil -> False
                    | Cons y ys -> d.(==) x y && xs == ys
    { (==) }

let monoid_Ordering = {
    (<>) = \x y ->
        case x of
            | EQ -> y
            | _ -> x,
    empty = EQ
}

type Ord a = {
    compare : a -> a -> Ordering
}

let ord_Int = {
    compare = \l r ->
        if l #Int< r
        then LT
        else if l #Int== r
        then EQ
        else GT
}

let ord_Float = {
    compare = \l r ->
        if l #Float< r
        then LT
        else if l #Float== r
        then EQ
        else GT
}
let ord_Option: Ord a -> Ord (Option a) = \compare_a -> {
    compare = \l r ->
        case l of
            | Some l_val ->
                (case r of
                    | Some r_val -> compare_a.compare l_val r_val
                    | None -> LT)
            | None -> 
                (case r of
                    | Some r_val -> GT
                    | None -> EQ)
}

let ord_Result: Ord e -> Ord t -> Ord (Result e t) = \ord_e ord_t -> {
    compare = \l r ->
        case l of
            | Ok l_val ->
                (case r of
                    | Ok r_val -> ord_t.compare l_val r_val
                    | Err _ -> GT)
            | Err l_val -> 
                (case r of
                    | Ok _ -> LT
                    | Err r_val -> ord_e.compare l_val r_val)
}

let make_Ord ord =
    let compare = ord.compare
    {
        (<=) = \l r ->
            case compare l r of
                | LT -> True
                | EQ -> True
                | GT -> False,
        (<) = \l r ->
            case compare l r of
                | LT -> True
                | EQ -> False
                | GT -> False,
        (>) = \l r ->
            case compare l r of
                | LT -> False
                | EQ -> False
                | GT -> True,
        (=>) = \l r ->
            case compare l r of
                | LT -> False
                | EQ -> True
                | GT -> True
    }

type Num a = {
    (+) : a -> a -> a,
    (-) : a -> a -> a,
    (*) : a -> a -> a,
    (/) : a -> a -> a,
    negate: a -> a
}

let num_Int: Num Int = {
    (+) = monoid_Int_Add.(<>),
    (-) = \l r -> l #Int- r,
    (*) = monoid_Int_Mul.(<>),
    (/) = \l r -> l #Int/ r,
    negate = \x -> 0 #Int- x
}

let num_Float: Num Float = {
    (+) = monoid_Float_Add.(<>),
    (-) = \l r -> l #Float- r,
    (*) = monoid_Float_Mul.(<>),
    (/) = \l r -> l #Float/ r,
    negate = \x -> 0.0 #Float- x
}

type Functor f = {
    map : (a -> b) -> f a -> f b
}

let functor_Option: Functor Option = {
    map = \f x -> case x of
                    | Some y -> Some (f y)
                    | None -> None
}

let functor_Result: Functor (Result e) = {
    map = \f x -> case x of
                    | Ok y -> Ok (f y)
                    | Err _ -> x
}
let functor_List: Functor List =
    let map f xs =
        case xs of
            | Cons y ys -> Cons (f y) (map f ys)
            | Nil -> Nil
    { map }

type Applicative f = {
    (<*>) : f (a -> b) -> f a -> f b,
    pure : a -> f a
}

let applicative_Option: Applicative Option = {
    (<*>) = \f x -> case f of
                        | Some g ->
                            (case x of
                                | Some y -> Some (g y)
                                | None -> None)
                        | None -> None,
    pure = \x -> Some x
}

let applicative_Result: Applicative (Result e) = {
    (<*>) = \f x -> case f of
                        | Ok g ->
                            (case x of
                                | Ok y -> Ok (g y)
                                | Err _ -> x)
                        | Err x -> Err x,
    pure = \x -> Ok x
}

let applicative_List: Applicative List =
    let (<*>) f xs =
            case f of
                | Cons g gs ->
                    monoid_List.(<>) (functor_List.map g xs) (gs <*> xs)
                | Nil -> Nil
    let pure x = Cons x Nil
    { (<*>), pure }

type Alternative f = {
    (<|>) : f a -> f a -> f a,
    empty : f a
}

let alternative_Option: Alternative Option = {
    (<|>) = \x y ->
        case x of
            | Some _ -> x
            | None -> y,
    empty = None
}

let alternative_List: Alternative List = {
    (<|>) = monoid_List.(<>),
    empty = Nil
}

let make_Alternative fun app alt =
    let { (<|>), empty } = alt
    let { (<*>), pure } = app
    let many x =
        let many_v _ = some_v () <|> pure Nil
        and some_v _ = fun.map (\h l -> Cons h l) x <*> many_v ()
        many_v ()
    let some x =
        let many_v _ = some_v () <|> pure Nil
        and some_v _ = fun.map (\h l -> Cons h l) x <*> many_v ()
        some_v ()
    {
        (<|>),
        empty,
        many,
        some
    }

type Monad m = {
    (>>=) : m a -> (a -> m b) -> m b,
    return : a -> m a
}

let monad_Option: Monad Option = {
    (>>=) = \m f -> case m of
                        | Some x -> f x
                        | None -> None,
    return = \x -> Some x
}

let monad_List: Monad List = {
    (>>=) = \m f -> concatMap f m,
    return = \x -> Cons x Nil
}

let monad_IO: Monad IO = {
    (>>=) = io_bind,
    return = io_return
}

let make_Monad m =
    let { (>>=), return } = m
    let (>>) l r = l >>= \_ -> r
    let forM_ xs f =
        case xs of
            | Cons y ys ->
                f y >> forM_ ys f
            | Nil -> return ()
    {
        (>>=),
        return = return,
        (>>),
        join = \mm -> mm >>= id,
        map = \x f -> x >>= (\y -> return (f x)),
        lift2 = \f lm rm -> lm >>= \l -> rm >>= \r -> f l r,
        forM_
    }

let functor_IO: Functor IO = {
    map = \f m1 -> monad_IO.(>>=) m1 (\x -> monad_IO.return (f x))
}

let applicative_IO: Applicative IO = {
    (<*>) = \f x ->
            monad_IO.(>>=) f (\g -> monad_IO.(>>=) x (\y -> monad_IO.return (g y))),
    pure = monad_IO.return
}

type Show a = {
    show : a -> String
}

let show_Unit: Show () = {
    show = const "()"
}

let show_Int: Show Int = {
    show = prim.show_Int
}

let show_Float: Show Float = {
    show = prim.show_Float
}

let (++) = string_prim.append

let show_List: Show a -> Show (List a) = \d ->
    let show xs =
        let show2 ys =
            case ys of
                | Cons y ys2 ->
                    case ys2 of
                        | Cons z zs -> d.show y ++ ", " ++ show2 ys2
                        | Nil -> d.show y ++ "]"
                | Nil -> "]"
        "[" ++ show2 xs
    { show }

let show_Option: Show a -> Show (Option a) = \d ->
    let show o =
        case o of
            | Some x -> "Some (" ++ d.show x ++ ")"
            | None -> "None"
    { show }

let show_Result: Show e -> Show t -> Show (Result e t) = \e t ->
    let show o =
        case o of
            | Ok x -> "Ok (" ++ t.show x ++ ")"
            | Err x -> "Err (" ++ e.show x ++ ")"
    { show }

{
    Eq,
    Ord,
    Monoid,
    Ordering,
    Option,
    Result,
    List,
    Functor,
    Applicative,
    Alternative,
    Monad,
    Num,
    Show,
    id, const, flip, not,
    foldl, foldr,
    ord_Option, ord_Result, ord_Float, ord_Int, make_Ord,
    eq_Unit, eq_List, eq_Option, eq_Result, eq_Float, eq_Int,
    monoid_Function, monoid_List, monoid_Option,
    monoid_Int_Add, monoid_Int_Mul, monoid_Float_Add, monoid_Float_Mul,
    num_Int, num_Float,
    functor_Option, functor_Result, functor_List, functor_IO,
    applicative_Option, applicative_Result, applicative_List, applicative_IO,
    alternative_Option, alternative_List,
    make_Alternative,
    monad_Option, monad_List, monad_IO,
    make_Monad,
    show_Unit, show_Int, show_Float, show_List, show_Option, show_Result
}

